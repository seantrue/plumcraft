// The WAVE the Windows app hands to MediaPlayer (N5 gate): a known sine in, the header, the length, the
// amplitude and the frequency out, so the conversion from the core's float32 samples can't drift.

using PlumCraft.Kit;
using Xunit;

namespace PlumCraft.Kit.Tests;

public class SoundTests
{
    static byte[] Sine(double hz, double amp, int rate, int frames, int ch)
    {
        var f = new float[frames * ch];
        for (int i = 0; i < frames; i++)
            for (int c = 0; c < ch; c++)
                f[i * ch + c] = (float)(amp * Math.Sin(2 * Math.PI * hz * i / rate) * (c == 0 ? 1 : 0.5));
        var b = new byte[f.Length * 4];
        Buffer.BlockCopy(f, 0, b, 0, b.Length);
        return b;
    }

    static short S16(byte[] w, int i) => BitConverter.ToInt16(w, 44 + i * 2);

    /// <summary>The strongest of the frequencies tried, by a direct Fourier sum over channel `c`.</summary>
    static double Peak(byte[] w, int ch, int c, int rate, IEnumerable<double> tries)
    {
        var frames = (w.Length - 44) / 2 / ch;
        double best = 0, at = 0;
        foreach (var hz in tries)
        {
            double re = 0, im = 0;
            for (int i = 0; i < frames; i++)
            {
                var x = S16(w, i * ch + c);
                re += x * Math.Cos(2 * Math.PI * hz * i / rate);
                im += x * Math.Sin(2 * Math.PI * hz * i / rate);
            }
            var m = re * re + im * im;
            if (m > best) { best = m; at = hz; }
        }
        return at;
    }

    [Theory]
    [InlineData(48000, 1)]
    [InlineData(44100, 2)]
    [InlineData(24000, 1)]
    public void ASineIsTheSameSineInTheWave(int rate, int ch)
    {
        var frames = rate / 2;
        var w = Sound.Wave(Sine(440, 0.5, rate, frames, ch), ch, frames, rate);
        // the header: RIFF, PCM, the channels, the rate, block align, 16 bits, the data's length
        Assert.Equal("RIFF", System.Text.Encoding.ASCII.GetString(w, 0, 4));
        Assert.Equal("WAVEfmt ", System.Text.Encoding.ASCII.GetString(w, 8, 8));
        Assert.Equal(1, BitConverter.ToInt16(w, 20));
        Assert.Equal(ch, BitConverter.ToInt16(w, 22));
        Assert.Equal(rate, BitConverter.ToInt32(w, 24));
        Assert.Equal(rate * ch * 2, BitConverter.ToInt32(w, 28));
        Assert.Equal(ch * 2, BitConverter.ToInt16(w, 32));
        Assert.Equal(16, BitConverter.ToInt16(w, 34));
        Assert.Equal(frames * ch * 2, BitConverter.ToInt32(w, 40));
        Assert.Equal(44 + frames * ch * 2, w.Length);
        // the amplitude: half of full scale, the second channel a quarter
        var peak0 = Enumerable.Range(0, frames).Max(i => Math.Abs((int)S16(w, i * ch)));
        Assert.InRange(peak0, 16383 - 2, 16384 + 2);
        if (ch > 1) Assert.InRange(Enumerable.Range(0, frames).Max(i => Math.Abs((int)S16(w, i * ch + 1))), 8190, 8194);
        // the frequency: 440 Hz, not an octave off (a channel count or rate mistake) or a buzz
        var hz = Peak(w, ch, 0, rate, [110, 220, 330, 400, 430, 440, 450, 480, 660, 880, 1760]);
        Assert.Equal(440, hz);
    }

    [Fact]
    public void OutOfRangeAndNotANumberAreHeldOrSilent()
    {
        var f = new float[] { 2f, -3f, float.NaN, float.PositiveInfinity, 0.25f };
        var b = new byte[f.Length * 4];
        Buffer.BlockCopy(f, 0, b, 0, b.Length);
        var w = Sound.Wave(b, 1, f.Length, 8000);
        Assert.Equal(new short[] { 32767, -32767, 0, 0, 8192 }, Enumerable.Range(0, f.Length).Select(i => S16(w, i)).ToArray());
        // fewer samples than frames × channels says: only whole frames
        Assert.Equal(44 + 8, Sound.Wave(b, 2, 10, 8000).Length);
    }
}
