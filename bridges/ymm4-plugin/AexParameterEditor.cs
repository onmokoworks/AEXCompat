using System.Collections.ObjectModel;
using System.ComponentModel.DataAnnotations;
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Data;
using System.Windows.Media;
using YukkuriMovieMaker.Commons;

namespace AEXCompat.Ymm4;

/// <summary>
/// Persisted, broker-shaped metadata and values for the parameter controls that
/// the first YMM4 GUI slice can safely transport.
/// </summary>
public sealed class AexParameterSet : Animatable
{
    public List<AexParameter> Items { get; set; } = [];

    [JsonIgnore]
    public string Status { get; set; } = "AEXパラメータを検出していません";

    internal static AexParameterSet Discover(string repository, string plugin)
    {
        if (string.IsNullOrWhiteSpace(repository) || string.IsNullOrWhiteSpace(plugin))
        {
            return new AexParameterSet
            {
                Status = "AEXCOMPAT_YMM4_REPOSITORY と AEXCOMPAT_YMM4_PLUGIN を設定してください",
            };
        }

        try
        {
            var buffer = new byte[1024 * 1024];
            var length = NativeMethods.Discover(repository, plugin, buffer, (nuint)buffer.Length);
            if (length < 0)
            {
                return new AexParameterSet { Status = ReadNativeError() };
            }

            var items = JsonSerializer.Deserialize<List<AexParameter>>(
                buffer.AsSpan(0, length), JsonOptions) ?? [];
            return new AexParameterSet
            {
                Items = items,
                Status = items.Count == 0
                    ? "編集可能なAEXパラメータはありません（AEXの既定値で描画します）"
                    : $"{items.Count}個のAEXパラメータを検出しました",
            };
        }
        catch (Exception ex)
        {
            return new AexParameterSet
            {
                Status = $"AEXパラメータ検出に失敗しました: {ex.Message}",
            };
        }
    }

    internal byte[] CreatePayload()
        => Items.Count == 0 ? [] : JsonSerializer.SerializeToUtf8Bytes(Items, JsonOptions);

    protected override IEnumerable<IAnimatable> GetAnimatables() => Items;

    private static string ReadNativeError()
    {
        var buffer = new byte[4096];
        var length = NativeMethods.LastError(0, buffer, (nuint)buffer.Length);
        var copiedLength = (int)Math.Min(length, (nuint)(buffer.Length - 1));
        return length == 0
            ? "AEXパラメータ検出に失敗しました（詳細なし）"
            : Encoding.UTF8.GetString(buffer, 0, copiedLength);
    }

    internal static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };
}

internal sealed class ByteArrayJsonConverter : JsonConverter<byte[]>
{
    public override byte[] Read(
        ref Utf8JsonReader reader,
        Type typeToConvert,
        JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return Convert.FromBase64String(reader.GetString() ?? string.Empty);
        }

        if (reader.TokenType != JsonTokenType.StartArray)
        {
            throw new JsonException("Expected a numeric byte array or a Base64 string.");
        }

        var values = new List<byte>();
        while (reader.Read() && reader.TokenType != JsonTokenType.EndArray)
        {
            if (reader.TokenType != JsonTokenType.Number || !reader.TryGetByte(out var value))
            {
                throw new JsonException("Expected byte values in the color array.");
            }

            values.Add(value);
        }

        if (reader.TokenType != JsonTokenType.EndArray)
        {
            throw new JsonException("The color array was not terminated.");
        }

        return values.ToArray();
    }

    public override void Write(
        Utf8JsonWriter writer,
        byte[] value,
        JsonSerializerOptions options)
    {
        writer.WriteStartArray();
        foreach (var item in value)
        {
            writer.WriteNumberValue(item);
        }

        writer.WriteEndArray();
    }
}

public sealed class AexParameter : Animatable
{
    [JsonPropertyName("slot")]
    public uint Slot { get; set; }

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("kind")]
    public string Kind { get; set; } = string.Empty;

    [JsonPropertyName("minimum")]
    public double Minimum { get; set; }

    [JsonPropertyName("maximum")]
    public double Maximum { get; set; }

    [JsonPropertyName("value")]
    public double Value { get; set; }

    [JsonPropertyName("choices")]
    public List<string> Choices { get; set; } = [];

    [JsonPropertyName("color")]
    [JsonConverter(typeof(ByteArrayJsonConverter))]
    public byte[] Color { get; set; } = [0, 0, 0, 255];

    [JsonPropertyName("components")]
    public double[] Components { get; set; } = [0, 0, 0];

    [JsonPropertyName("component_count")]
    public int ComponentCount { get; set; }

    [JsonPropertyName("layer_path")]
    public string? LayerPath { get; set; }

    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; }

    [JsonPropertyName("visible")]
    public bool Visible { get; set; }

    [JsonPropertyName("supervised")]
    public bool Supervised { get; set; }

    [JsonPropertyName("debug_summary")]
    public string? DebugSummary { get; set; }

    [JsonPropertyName("custom_ui_events")]
    public uint CustomUiEvents { get; set; }

    [JsonPropertyName("control_size")]
    public ushort[] ControlSize { get; set; } = [0, 0];

    [JsonIgnore]
    public string DisplayName => string.IsNullOrWhiteSpace(Name) ? $"Slot {Slot}" : Name;

    [JsonIgnore]
    public bool IsPopup => Choices.Count > 0;

    [JsonIgnore]
    public bool IsCheckbox => !IsPopup && Kind == "integer" && Minimum == 0 && Maximum == 1;

    [JsonIgnore]
    public bool IsColor => Kind == "color";

    [JsonIgnore]
    public bool IsSlider => Kind is "float" or "integer" && !IsPopup && !IsCheckbox;

    protected override IEnumerable<IAnimatable> GetAnimatables() => [];
}

internal sealed class AexParameterEditorAttribute : PropertyEditorAttribute2
{
    public override FrameworkElement Create() => new AexParameterEditorControl();

    public override void SetBindings(FrameworkElement control, ItemProperty[] itemProperties)
    {
        var editor = (AexParameterEditorControl)control;
        editor.Attach(itemProperties.FirstOrDefault()?.PropertyOwner as AexCompatVideoEffect);
    }

    public override void ClearBindings(FrameworkElement control)
        => ((AexParameterEditorControl)control).Attach(null);
}

internal sealed class AexParameterEditorControl : UserControl, IPropertyEditorControl
{
    private readonly StackPanel panel = new();
    private AexCompatVideoEffect? effect;

    public event EventHandler? BeginEdit;
    public event EventHandler? EndEdit;

    public AexParameterEditorControl()
    {
        Content = new ScrollViewer
        {
            Content = panel,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            MaxHeight = 420,
        };
    }

    public void Attach(AexCompatVideoEffect? next)
    {
        if (effect is not null)
        {
            effect.PropertyChanged -= EffectOnPropertyChanged;
        }
        effect = next;
        if (effect is not null)
        {
            effect.PropertyChanged += EffectOnPropertyChanged;
        }
        Rebuild();
    }

    private void EffectOnPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(AexCompatVideoEffect.Parameters)
            or nameof(AexCompatVideoEffect.PluginPath)
            or nameof(AexCompatVideoEffect.RepositoryPath))
        {
            Rebuild();
        }
    }

    private void Rebuild()
    {
        panel.Children.Clear();
        if (effect is null)
        {
            panel.Children.Add(Label("AEXCompatのエフェクトを選択してください"));
            return;
        }

        panel.Children.Add(Label(effect.Parameters.Status));
        foreach (var parameter in effect.Parameters.Items)
        {
            if (!parameter.Visible)
            {
                continue;
            }
            panel.Children.Add(CreateParameterRow(parameter));
        }
    }

    private FrameworkElement CreateParameterRow(AexParameter parameter)
    {
        var row = new Grid { Margin = new Thickness(0, 2, 0, 2) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(150) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var name = new TextBlock
        {
            Text = parameter.DisplayName,
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            ToolTip = parameter.DebugSummary,
        };
        row.Children.Add(name);

        FrameworkElement editor = parameter.IsPopup
            ? CreatePopup(parameter)
            : parameter.IsCheckbox
                ? CreateCheckbox(parameter)
                : parameter.IsColor
                    ? CreateColor(parameter)
                    : CreateSlider(parameter);
        Grid.SetColumn(editor, 1);
        row.Children.Add(editor);
        return row;
    }

    private FrameworkElement CreatePopup(AexParameter parameter)
    {
        var combo = new ComboBox
        {
            ItemsSource = parameter.Choices,
            SelectedIndex = Math.Clamp((int)Math.Round(parameter.Value) - 1, 0, parameter.Choices.Count - 1),
            MinWidth = 120,
        };
        combo.SelectionChanged += (_, _) =>
        {
            if (combo.SelectedIndex >= 0)
            {
                BeginEdit?.Invoke(this, EventArgs.Empty);
                parameter.Value = combo.SelectedIndex + 1;
                EndEdit?.Invoke(this, EventArgs.Empty);
            }
        };
        return combo;
    }

    private FrameworkElement CreateCheckbox(AexParameter parameter)
    {
        var checkBox = new CheckBox { IsChecked = parameter.Value != 0 };
        checkBox.Checked += (_, _) => SetParameterValue(parameter, 1);
        checkBox.Unchecked += (_, _) => SetParameterValue(parameter, 0);
        return checkBox;
    }

    private FrameworkElement CreateSlider(AexParameter parameter)
    {
        var dock = new DockPanel();
        var text = new TextBox
        {
            Text = parameter.Value.ToString("G6", CultureInfo.InvariantCulture),
            Width = 75,
            Margin = new Thickness(6, 0, 0, 0),
            VerticalContentAlignment = VerticalAlignment.Center,
        };
        DockPanel.SetDock(text, Dock.Right);
        var slider = new Slider
        {
            Minimum = parameter.Minimum,
            Maximum = parameter.Maximum,
            Value = Math.Clamp(parameter.Value, parameter.Minimum, parameter.Maximum),
            IsSnapToTickEnabled = parameter.Kind == "integer",
            TickFrequency = parameter.Kind == "integer" ? 1 : Math.Max((parameter.Maximum - parameter.Minimum) / 100, 0.001),
        };
        var updating = false;
        slider.ValueChanged += (_, args) =>
        {
            if (updating)
            {
                return;
            }
            updating = true;
            text.Text = args.NewValue.ToString("G6", CultureInfo.InvariantCulture);
            updating = false;
            SetParameterValue(parameter, parameter.Kind == "integer" ? Math.Round(args.NewValue) : args.NewValue);
        };
        text.LostFocus += (_, _) =>
        {
            if (!double.TryParse(text.Text, NumberStyles.Float, CultureInfo.InvariantCulture, out var value))
            {
                text.Text = slider.Value.ToString("G6", CultureInfo.InvariantCulture);
                return;
            }
            value = Math.Clamp(value, parameter.Minimum, parameter.Maximum);
            updating = true;
            slider.Value = value;
            text.Text = value.ToString("G6", CultureInfo.InvariantCulture);
            updating = false;
            SetParameterValue(parameter, parameter.Kind == "integer" ? Math.Round(value) : value);
        };
        dock.Children.Add(slider);
        dock.Children.Add(text);
        return dock;
    }

    private FrameworkElement CreateColor(AexParameter parameter)
    {
        var dock = new DockPanel();
        var text = new TextBox
        {
            Text = ToColorText(parameter.Color),
            MinWidth = 100,
        };
        text.LostFocus += (_, _) =>
        {
            if (TryParseColor(text.Text, out var color))
            {
                BeginEdit?.Invoke(this, EventArgs.Empty);
                parameter.Color = color;
                EndEdit?.Invoke(this, EventArgs.Empty);
            }
            else
            {
                text.Text = ToColorText(parameter.Color);
            }
        };
        dock.Children.Add(text);
        return dock;
    }

    private void SetParameterValue(AexParameter parameter, double value)
    {
        BeginEdit?.Invoke(this, EventArgs.Empty);
        parameter.Value = Math.Clamp(value, parameter.Minimum, parameter.Maximum);
        EndEdit?.Invoke(this, EventArgs.Empty);
    }

    private static TextBlock Label(string text)
        => new() { Text = text, Margin = new Thickness(0, 2, 0, 6), TextWrapping = TextWrapping.Wrap };

    private static string ToColorText(byte[] color)
        => color.Length >= 4 ? $"#{color[0]:X2}{color[1]:X2}{color[2]:X2}{color[3]:X2}" : "#FFFFFFFF";

    private static bool TryParseColor(string? text, out byte[] color)
    {
        color = [0, 0, 0, 255];
        var value = text?.Trim().TrimStart('#');
        if (value is null || value.Length != 8 || !uint.TryParse(value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var packed))
        {
            return false;
        }
        color = [(byte)(packed >> 24), (byte)(packed >> 16), (byte)(packed >> 8), (byte)packed];
        return true;
    }
}
