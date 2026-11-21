// A SynchronizationContext with a thread of its own: the headless stand-in for a window's UI thread.
//
// The document model and the canvas link are touched on one context only (Swift's @MainActor). In
// an app that is the UI thread's (WinUI's DispatcherQueueSynchronizationContext); in tests and
// headless tools it is one of these. Every await in code started on it resumes on it, so the model's
// state is only ever touched by its thread.

using System.Collections.Concurrent;

namespace PlumCraft.Kit;

public sealed class SerialContext : SynchronizationContext, IDisposable
{
    private readonly BlockingCollection<(SendOrPostCallback d, object? state)> queue = new();
    private readonly Thread thread;

    /// <summary>A callback's exception (the thread goes on); the last one, for tests.</summary>
    public Exception? LastFault { get; private set; }
    public event Action<Exception>? Fault;

    public SerialContext(string name = "plum kit")
    {
        thread = new Thread(Loop) { IsBackground = true, Name = name };
        thread.Start();
    }

    /// <summary>Whether the caller is on this context's thread.</summary>
    public bool IsCurrent => Thread.CurrentThread == thread;

    private void Loop()
    {
        SetSynchronizationContext(this);
        foreach (var (d, state) in queue.GetConsumingEnumerable())
        {
            try
            {
                d(state);
            }
            catch (Exception e)
            {
                LastFault = e;
                Fault?.Invoke(e);
            }
        }
    }

    public override void Post(SendOrPostCallback d, object? state)
    {
        try { queue.Add((d, state)); }
        catch (InvalidOperationException) { /* disposed: dropped */ }
    }

    public override void Send(SendOrPostCallback d, object? state)
    {
        if (IsCurrent)
        {
            d(state);
            return;
        }
        using var done = new ManualResetEventSlim();
        Exception? fault = null;
        Post(_ =>
        {
            try { d(state); }
            catch (Exception e) { fault = e; }
            finally { done.Set(); }
        }, null);
        if (!done.Wait(TimeSpan.FromSeconds(60))) throw new TimeoutException("the context didn't run the callback in 60 s");
        if (fault is not null) throw new AggregateException(fault);
    }

    public override SynchronizationContext CreateCopy() => this;

    /// <summary>Run an async function on this context; its awaits resume here.</summary>
    public Task<T> Run<T>(Func<Task<T>> f)
    {
        var tcs = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
        Post(async _ =>
        {
            try { tcs.SetResult(await f()); }
            catch (Exception e) { tcs.SetException(e); }
        }, null);
        return tcs.Task;
    }

    public Task Run(Func<Task> f) => Run(async () =>
    {
        await f();
        return true;
    });

    public void Dispose()
    {
        queue.CompleteAdding();
        if (!IsCurrent) thread.Join(5000);
    }
}
