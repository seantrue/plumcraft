// A sound's samples made into what Windows' MediaPlayer plays (N5 slice 4's AudioViewer, moved here in
// the N5 gate so a test pins it): the samples the core lends (`media.get {want: "bytes"}` on an AUDIO
// value: float32, frames by channels, interleaved) as a 16-bit PCM WAVE at the sound's own rate.

using System.Text;

namespace PlumCraft.Kit;

public static class Sound
{
    /// <summary>16-bit PCM WAVE from interleaved float32 samples (little-endian), each held to -1..1;
    /// a sample that isn't a finite number is silence. At most `frames` × `ch` samples are taken.</summary>
    public static byte[] Wave(byte[] f32, int ch, int frames, int rate)
    {
        ch = Math.Max(1, ch);
        var n = Math.Max(0, Math.Min(f32.Length / 4, frames * ch));
        n -= n % ch;
        var data = n * 2;
        var o = new byte[44 + data];
        void W32(int at, int v) => BitConverter.TryWriteBytes(o.AsSpan(at, 4), v);
        void W16(int at, short v) => BitConverter.TryWriteBytes(o.AsSpan(at, 2), v);
        Encoding.ASCII.GetBytes("RIFF").CopyTo(o, 0);
        W32(4, 36 + data);
        Encoding.ASCII.GetBytes("WAVEfmt ").CopyTo(o, 8);
        W32(16, 16);
        W16(20, 1);
        W16(22, (short)ch);
        W32(24, rate);
        W32(28, rate * ch * 2);
        W16(32, (short)(ch * 2));
        W16(34, 16);
        Encoding.ASCII.GetBytes("data").CopyTo(o, 36);
        W32(40, data);
        for (int i = 0; i < n; i++)
        {
            var x = BitConverter.ToSingle(f32, i * 4);
            if (!float.IsFinite(x)) x = 0;
            W16(44 + i * 2, (short)Math.Round(Math.Clamp(x, -1f, 1f) * 32767));
        }
        return o;
    }
}
