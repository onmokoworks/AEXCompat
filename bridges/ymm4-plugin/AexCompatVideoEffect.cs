using System.Runtime.InteropServices;
using System.Text;
using System.ComponentModel;
using System.IO;
using System.Reflection;
using System.Numerics;
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
        repositoryPath = AexCompatRuntime.ResolveRepositoryPath();
        Parameters = AexParameterSet.Discover(repositoryPath, pluginPath);
        Remark = "AEXファイルを指定すると、対応するパラメータをこのエフェクトのGUIから編集できます";
    }

    public override string Label => "AEXCompat";

    [Display(GroupName = "AEXCompat", Name = "AEXファイル", Description = "読み込むAfter Effectsプラグインのパス")]
    [Browsable(false)]
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

    [Display(GroupName = "AEXCompat", Name = "AEXCompatリポジトリ", Description = "aex_worker.exeを含むAEXCompatリポジトリのパス")]
    [Browsable(false)]
    public string RepositoryPath
    {
        get => repositoryPath;
        set
        {
            if (string.Equals(repositoryPath, value, StringComparison.OrdinalIgnoreCase))
            {
                return;
            }
            repositoryPath = value ?? string.Empty;
            OnPropertyChanged(nameof(RepositoryPath));
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
    private readonly ID2D1DeviceContext6 deviceContext;
    private readonly AexCompatVideoEffect effect;
    private readonly ID2D1Effect outputTransform;
    private readonly ID2D1Image transformedOutput;
    private readonly object gate = new();
    private ID2D1Image? input;
    private ID2D1Bitmap1? output;
    private nint session;
    private int sessionWidth;
    private int sessionHeight;
    private int sessionFps;
    private int sessionDuration;
    private string? sessionPluginPath;
    private string? sessionRepositoryPath;
    private uint frameSerial;
    private string? lastError;

    public AexCompatVideoEffectProcessor(IGraphicsDevicesAndContext devices, AexCompatVideoEffect effect)
    {
        deviceContext = devices.DeviceContext;
        this.effect = effect;
        outputTransform = new ID2D1Effect(deviceContext.CreateEffect(EffectGuids.AffineTransform2D));
        transformedOutput = outputTransform.Output;
    }

    public ID2D1Image Output => output is null ? input! : transformedOutput;

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

                UpdateOutputBitmap(rgbaOutput, size);
                lastError = null;
            }
        }
        catch (Exception ex)
        {
            lock (gate)
            {
                if (IsSessionFatal(ex.Message))
                {
                    CloseSession();
                }

                UseInput(ex.Message);
            }
        }

        return effectDescription.DrawDescription;
    }

    private static bool IsSessionFatal(string message)
    {
        return message.StartsWith("YMM4 render reply exceeded", StringComparison.Ordinal)
            || message.StartsWith("YMM4 render reply lost", StringComparison.Ordinal)
            || message.StartsWith("YMM4 session thread stopped", StringComparison.Ordinal)
            || message.StartsWith("YMM4 session is closed", StringComparison.Ordinal);
    }

    public void Dispose()
    {
        lock (gate)
        {
            input = null;
            outputTransform.SetInput(0, null, true);
            CloseSession();
            ReplaceOutput(null);
            transformedOutput.Dispose();
            outputTransform.Dispose();
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
        if (session != 0 && (sessionWidth != width || sessionHeight != height || sessionFps != fps || sessionDuration != duration || !string.Equals(sessionPluginPath, pluginPath, StringComparison.OrdinalIgnoreCase) || !string.Equals(sessionRepositoryPath, repositoryPath, StringComparison.OrdinalIgnoreCase)))
        {
            CloseSession();
        }
        if (session != 0)
        {
            return;
        }
        if (string.IsNullOrWhiteSpace(pluginPath) || string.IsNullOrWhiteSpace(repositoryPath))
        {
            throw new InvalidOperationException("AEXファイルとAEXCompat実行フォルダを指定してください");
        }
        session = NativeMethods.Open(
            repositoryPath,
            pluginPath,
            checked((uint)width),
            checked((uint)height),
            timeStep: 1,
            totalTime: duration,
            timeScale: checked((uint)fps),
            smart: 1);
        if (session == 0)
        {
            throw new InvalidOperationException(ReadNativeError());
        }
        sessionWidth = width;
        sessionHeight = height;
        sessionFps = fps;
        sessionDuration = duration;
        sessionPluginPath = pluginPath;
        sessionRepositoryPath = repositoryPath;
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
        var previousTransform = deviceContext.Transform;
        try
        {
            deviceContext.Target = target;
            deviceContext.Transform = new Matrix3x2(
                1,
                0,
                0,
                1,
                size.Width / 2f,
                size.Height / 2f);
            deviceContext.BeginDraw();
            deviceContext.DrawImage(input!, InterpolationMode.NearestNeighbor, CompositeMode.SourceOver);
            deviceContext.EndDraw();
        }
        finally
        {
            deviceContext.Transform = previousTransform;
            deviceContext.Target = previousTarget;
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

    private void UpdateOutputBitmap(byte[] rgba, System.Drawing.Size size)
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
            if (output is null
                || output.PixelSize.Width != size.Width
                || output.PixelSize.Height != size.Height)
            {
                var format = new PixelFormat(Format.B8G8R8A8_UNorm, D2DAlphaMode.Premultiplied);
                var next = deviceContext.CreateBitmap(
                    new SizeI(size.Width, size.Height),
                    handle.AddrOfPinnedObject(),
                    size.Width * 4,
                    new BitmapProperties1(format, 96, 96, BitmapOptions.None));
                var previous = output;
                output = next;
                previous?.Dispose();
                outputTransform.SetInput(0, output, true);
            }
            else
            {
                output.CopyFromMemory(handle.AddrOfPinnedObject(), size.Width * 4);
            }

            outputTransform.SetValue(
                (int)AffineTransform2DProperties.TransformMatrix,
                new Matrix3x2(1, 0, 0, 1, -size.Width / 2f, -size.Height / 2f));
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
        if (next is null)
        {
            outputTransform.SetInput(0, null, true);
        }
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
        sessionRepositoryPath = null;
    }

    private static byte Premultiply(byte value, byte alpha)
        => (byte)((value * alpha + 127) / 255);

    private static byte Unpremultiply(byte value, byte alpha)
        => alpha == 0 ? (byte)0 : (byte)Math.Min(255, (value * 255 + alpha / 2) / alpha);
}

internal static partial class NativeMethods
{
    private const string NativeLibraryName = "aexcompat_ymm4_native.dll";

    static NativeMethods()
    {
        NativeLibrary.SetDllImportResolver(typeof(NativeMethods).Assembly, ResolveNativeLibrary);
    }

    private static nint ResolveNativeLibrary(
        string libraryName,
        Assembly assembly,
        DllImportSearchPath? searchPath)
    {
        if (!string.Equals(libraryName, NativeLibraryName, StringComparison.OrdinalIgnoreCase))
        {
            return nint.Zero;
        }

        var path = Path.Combine(AexCompatRuntime.AssemblyDirectory, NativeLibraryName);
        return File.Exists(path) ? NativeLibrary.Load(path) : nint.Zero;
    }

    [LibraryImport(NativeLibraryName, EntryPoint = "aexcompat_ymm4_open", StringMarshalling = StringMarshalling.Utf16)]
    internal static partial nint Open(
        string repository,
        string plugin,
        uint width,
        uint height,
        int timeStep,
        int totalTime,
        uint timeScale,
        byte smart);

    [LibraryImport(NativeLibraryName, EntryPoint = "aexcompat_ymm4_discover", StringMarshalling = StringMarshalling.Utf16)]
    [return: MarshalAs(UnmanagedType.I4)]
    internal static partial int Discover(
        string repository,
        string plugin,
        [Out] byte[] output,
        nuint outputLength);

    [LibraryImport(NativeLibraryName, EntryPoint = "aexcompat_ymm4_render")]
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

    [LibraryImport(NativeLibraryName, EntryPoint = "aexcompat_ymm4_last_error")]
    internal static partial nuint LastError(nint session, [Out] byte[] output, nuint outputLength);

    [LibraryImport(NativeLibraryName, EntryPoint = "aexcompat_ymm4_close")]
    internal static partial void Close(nint session);
}

internal static class AexCompatRuntime
{
    private const string WorkerRelativePath = "target\\minihost-build\\aex_worker.exe";

    public static string AssemblyDirectory
        => Path.GetDirectoryName(typeof(AexCompatRuntime).Assembly.Location)
            ?? AppContext.BaseDirectory;

    public static string ResolveRepositoryPath()
    {
        foreach (var candidate in CandidateRoots())
        {
            if (File.Exists(Path.Combine(candidate, WorkerRelativePath)))
            {
                return candidate;
            }
        }

        var configured = Environment.GetEnvironmentVariable("AEXCOMPAT_YMM4_REPOSITORY");
        return string.IsNullOrWhiteSpace(configured) ? string.Empty : configured;
    }

    private static IEnumerable<string> CandidateRoots()
    {
        var current = new DirectoryInfo(AssemblyDirectory);
        for (var depth = 0; current is not null && depth < 8; depth++, current = current.Parent)
        {
            yield return current.FullName;
        }
    }
}
