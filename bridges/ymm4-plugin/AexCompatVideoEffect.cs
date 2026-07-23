using System.Runtime.InteropServices;
using System.Text;
using System.ComponentModel.DataAnnotations;
using Vortice.DCommon;
using Vortice.Direct2D1;
using Vortice.DXGI;
using Vortice.Mathematics;
using YukkuriMovieMaker.Commons;
using YukkuriMovieMaker.Plugin.Effects;
using YukkuriMovieMaker.Player.Video;

namespace AEXCompat.Ymm4;

using D2DAlphaMode = Vortice.DCommon.AlphaMode;

[VideoEffect("AEXCompat", ["AEXCompat"], ["AEX", "After Effects"])]
public sealed class AexCompatVideoEffect : VideoEffectBase
{
    private string pluginPath;
    private string repositoryPath;
    private AexParameterSet parameters = new();

    public AexCompatVideoEffect()
    {
        pluginPath = Environment.GetEnvironmentVariable("AEXCOMPAT_YMM4_PLUGIN") ?? string.Empty;
        repositoryPath = Environment.GetEnvironmentVariable("AEXCOMPAT_YMM4_REPOSITORY") ?? string.Empty;
        Parameters = AexParameterSet.Discover(repositoryPath, pluginPath);
        Remark = "AEXファイルを指定すると、対応するパラメータをこのエフェクトのGUIから編集できます";
    }

    public override string Label => "AEXCompat";

    [Display(GroupName = "AEXCompat", Name = "AEXファイル", Description = "読み込むAfter Effectsプラグインのパス")]
    [YukkuriMovieMaker.Controls.TextEditor]
    public string PluginPath
    {
        get => pluginPath;
        set
        {
            if (string.Equals(pluginPath, value, StringComparison.OrdinalIgnoreCase))
            {
                return;
            }
            pluginPath = value ?? string.Empty;
            OnPropertyChanged(nameof(PluginPath));
            SetParameters(AexParameterSet.Discover(repositoryPath, pluginPath));
        }
    }

    [Display(GroupName = "AEXCompat", Name = "AEXパラメータ", Description = "検出された対応パラメータ")]
    [AexParameterEditor]
    public AexParameterSet Parameters
    {
        get => parameters;
        set => SetParameters(value ?? new AexParameterSet());
    }

    public string RepositoryPath => repositoryPath;

    protected override IEnumerable<IAnimatable> GetAnimatables() => [Parameters];

    internal byte[] CreateParameterPayload()
        => Parameters.CreatePayload();

    private void SetParameters(AexParameterSet next)
    {
        parameters = next;
        OnPropertyChanged(nameof(Parameters));
    }

    public override IVideoEffectProcessor CreateVideoEffect(IGraphicsDevicesAndContext devices)
        => new AexCompatVideoEffectProcessor(devices, this);

    public override IEnumerable<string> CreateExoVideoFilters(
        int keyFrameIndex,
        YukkuriMovieMaker.Exo.ExoOutputDescription exoOutputDescription)
        => [];
}

internal sealed class AexCompatVideoEffectProcessor : IVideoEffectProcessor
{
    private const string PluginEnvironment = "AEXCOMPAT_YMM4_PLUGIN";
    private const string RepositoryEnvironment = "AEXCOMPAT_YMM4_REPOSITORY";

    private readonly ID2D1DeviceContext6 deviceContext;
    private readonly AexCompatVideoEffect effect;
    private readonly object gate = new();
    private ID2D1Image? input;
    private ID2D1Bitmap1? output;
    private nint session;
    private int sessionWidth;
    private int sessionHeight;
    private int sessionFps;
    private int sessionDuration;
    private string? sessionPluginPath;
    private uint frameSerial;
    private string? lastError;

    public AexCompatVideoEffectProcessor(IGraphicsDevicesAndContext devices, AexCompatVideoEffect effect)
    {
        deviceContext = devices.DeviceContext;
        this.effect = effect;
    }

    public ID2D1Image Output => output ?? input!;

    public void SetInput(ID2D1Image? input)
    {
        this.input = input;
    }

    public void ClearInput()
    {
        input = null;
    }

    public DrawDescription Update(EffectDescription effectDescription)
    {
        if (input is null)
        {
            return effectDescription.DrawDescription;
        }

        var size = effectDescription.ScreenSize;
        if (size.Width <= 0 || size.Height <= 0 || size.Width > 4096 || size.Height > 4096)
        {
            UseInput("YMM4 frame dimensions are outside the bridge limits");
            return effectDescription.DrawDescription;
        }

        var fps = Math.Max(1, effectDescription.FPS);
        var duration = Math.Max(1, effectDescription.ItemDuration.Frame);
        var currentTime = Math.Clamp(effectDescription.ItemPosition.Frame, 0, int.MaxValue);

        try
        {
            lock (gate)
            {
                var pluginPath = effect.PluginPath;
                var repositoryPath = effect.RepositoryPath;
                EnsureSession(size.Width, size.Height, fps, duration, pluginPath, repositoryPath);
                var rgbaInput = ReadInput(size);
                var rgbaOutput = new byte[rgbaInput.Length];
                var parameters = effect.CreateParameterPayload();
                var outputWidth = 0u;
                var outputHeight = 0u;
                var result = NativeMethods.Render(
                    session,
                    frameSerial++,
                    currentTime,
                    rgbaInput,
                    (nuint)rgbaInput.Length,
                    rgbaOutput,
                    (nuint)rgbaOutput.Length,
                    ref outputWidth,
                    ref outputHeight,
                    parameters,
                    (nuint)parameters.Length);
                if (result != 0)
                {
                    throw new InvalidOperationException(ReadNativeError());
                }
                if (outputWidth != size.Width || outputHeight != size.Height)
                {
                    throw new InvalidOperationException(
                        $"AEX changed frame size to {outputWidth}x{outputHeight}; YMM4 bridge requires fixed size");
                }

                ReplaceOutput(CreateOutputBitmap(rgbaOutput, size));
                lastError = null;
            }
        }
        catch (Exception ex)
        {
            UseInput(ex.Message);
        }

        return effectDescription.DrawDescription;
    }

    public void Dispose()
    {
        lock (gate)
        {
            CloseSession();
            ReplaceOutput(null);
        }
    }

    private void EnsureSession(
        int width,
        int height,
        int fps,
        int duration,
        string pluginPath,
        string repositoryPath)
    {
        if (session != 0 && (sessionWidth != width || sessionHeight != height || sessionFps != fps || sessionDuration != duration || !string.Equals(sessionPluginPath, pluginPath, StringComparison.OrdinalIgnoreCase)))
        {
            CloseSession();
        }
        if (session != 0)
        {
            return;
        }
        if (string.IsNullOrWhiteSpace(pluginPath) || string.IsNullOrWhiteSpace(repositoryPath))
        {
            throw new InvalidOperationException(
                $"Set {PluginEnvironment} and {RepositoryEnvironment} before using the effect");
        }
        session = NativeMethods.Open(
            repositoryPath,
            pluginPath,
            checked((uint)width),
            checked((uint)height),
            timeStep: 1,
            totalTime: duration,
            timeScale: checked((uint)fps),
            smart: 0);
        if (session == 0)
        {
            throw new InvalidOperationException(ReadNativeError());
        }
        sessionWidth = width;
        sessionHeight = height;
        sessionFps = fps;
        sessionDuration = duration;
        sessionPluginPath = pluginPath;
        frameSerial = 0;
    }

    private byte[] ReadInput(System.Drawing.Size size)
    {
        var format = new PixelFormat(Format.B8G8R8A8_UNorm, D2DAlphaMode.Premultiplied);
        using var target = deviceContext.CreateBitmap(
            new SizeI(size.Width, size.Height),
            new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
        using var readable = deviceContext.CreateBitmap(
            new SizeI(size.Width, size.Height),
            new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));

        var previousTarget = deviceContext.Target;
        try
        {
            deviceContext.Target = target;
            deviceContext.BeginDraw();
            deviceContext.DrawImage(input!, InterpolationMode.NearestNeighbor, CompositeMode.SourceOver);
            deviceContext.EndDraw();
        }
        finally
        {
            deviceContext.Target = previousTarget;
            previousTarget?.Dispose();
        }
        readable.CopyFromBitmap(target);
        var mapped = readable.Map(MapOptions.Read);
        try
        {
            var rgba = new byte[size.Width * size.Height * 4];
            var row = new byte[size.Width * 4];
            for (var y = 0; y < size.Height; y++)
            {
                Marshal.Copy(IntPtr.Add(mapped.Bits, y * mapped.Pitch), row, 0, row.Length);
                for (var x = 0; x < size.Width; x++)
                {
                    var source = x * 4;
                    var destination = (y * size.Width + x) * 4;
                    var alpha = row[source + 3];
                    rgba[destination] = Unpremultiply(row[source + 2], alpha);
                    rgba[destination + 1] = Unpremultiply(row[source + 1], alpha);
                    rgba[destination + 2] = Unpremultiply(row[source], alpha);
                    rgba[destination + 3] = alpha;
                }
            }
            return rgba;
        }
        finally
        {
            readable.Unmap();
        }
    }

    private ID2D1Bitmap1 CreateOutputBitmap(byte[] rgba, System.Drawing.Size size)
    {
        var bgra = new byte[rgba.Length];
        for (var index = 0; index < rgba.Length; index += 4)
        {
            var alpha = rgba[index + 3];
            bgra[index] = Premultiply(rgba[index + 2], alpha);
            bgra[index + 1] = Premultiply(rgba[index + 1], alpha);
            bgra[index + 2] = Premultiply(rgba[index], alpha);
            bgra[index + 3] = alpha;
        }

        var handle = GCHandle.Alloc(bgra, GCHandleType.Pinned);
        try
        {
            var format = new PixelFormat(Format.B8G8R8A8_UNorm, D2DAlphaMode.Premultiplied);
            return deviceContext.CreateBitmap(
                new SizeI(size.Width, size.Height),
                handle.AddrOfPinnedObject(),
                size.Width * 4,
                new BitmapProperties1(format, 96, 96, BitmapOptions.None));
        }
        finally
        {
            handle.Free();
        }
    }

    private string ReadNativeError()
    {
        var buffer = new byte[4096];
        var length = NativeMethods.LastError(session, buffer, (nuint)buffer.Length);
        var copiedLength = (int)Math.Min(length, (nuint)(buffer.Length - 1));
        return length == 0
            ? "AEXCompat YMM4 native bridge failed without a diagnostic"
            : Encoding.UTF8.GetString(buffer, 0, copiedLength);
    }

    private void UseInput(string error)
    {
        lastError = error;
        ReplaceOutput(null);
    }

    private void ReplaceOutput(ID2D1Bitmap1? next)
    {
        output?.Dispose();
        output = next;
    }

    private void CloseSession()
    {
        if (session != 0)
        {
            NativeMethods.Close(session);
            session = 0;
        }
        sessionPluginPath = null;
    }

    private static byte Premultiply(byte value, byte alpha)
        => (byte)((value * alpha + 127) / 255);

    private static byte Unpremultiply(byte value, byte alpha)
        => alpha == 0 ? (byte)0 : (byte)Math.Min(255, (value * 255 + alpha / 2) / alpha);
}

internal static partial class NativeMethods
{
    [LibraryImport("aexcompat_ymm4_native.dll", EntryPoint = "aexcompat_ymm4_open", StringMarshalling = StringMarshalling.Utf16)]
    internal static partial nint Open(
        string repository,
        string plugin,
        uint width,
        uint height,
        int timeStep,
        int totalTime,
        uint timeScale,
        byte smart);

    [LibraryImport("aexcompat_ymm4_native.dll", EntryPoint = "aexcompat_ymm4_discover", StringMarshalling = StringMarshalling.Utf16)]
    [return: MarshalAs(UnmanagedType.I4)]
    internal static partial int Discover(
        string repository,
        string plugin,
        [Out] byte[] output,
        nuint outputLength);

    [LibraryImport("aexcompat_ymm4_native.dll", EntryPoint = "aexcompat_ymm4_render")]
    [return: MarshalAs(UnmanagedType.I4)]
    internal static partial int Render(
        nint session,
        uint frameIndex,
        int currentTime,
        [In] byte[] rgba,
        nuint rgbaLength,
        [Out] byte[] output,
        nuint outputLength,
        ref uint outputWidth,
        ref uint outputHeight,
        [In] byte[] parameters,
        nuint parametersLength);

    [LibraryImport("aexcompat_ymm4_native.dll", EntryPoint = "aexcompat_ymm4_last_error")]
    internal static partial nuint LastError(nint session, [Out] byte[] output, nuint outputLength);

    [LibraryImport("aexcompat_ymm4_native.dll", EntryPoint = "aexcompat_ymm4_close")]
    internal static partial void Close(nint session);
}
