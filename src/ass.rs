
// src/ass.rs
// 提供弹幕输入结构和 ASS 字幕渲染逻辑.

use std::{cmp::Ordering, fmt::Write};

use anyhow::Context;
use serde_json::Value;

const PLAY_RES_X: i32 = 1920;
const PLAY_RES_Y: i32 = 1080;
const FIXED_DURATION: f64 = 5.0;

/// 生成 ASS 字幕时使用的样式和时间设置.
#[derive(Debug, Clone)]
pub struct AssSettings {
    /// ASS 字体名称.
    pub font: String,
    /// 用户侧字体大小, 会映射到 ASS 坐标.
    pub font_size: f64,
    /// 弹幕不透明度, 范围为 0..=1.
    pub alpha: f64,
    /// 是否使用粗体.
    pub bold: bool,
    /// 弹幕描边宽度.
    pub border: f64,
    /// 弹幕时间偏移, 正值延后, 负值提前.
    pub offset: f64,
    /// 滚动弹幕横穿画面的持续时间, 单位为秒.
    pub scroll_duration: f64,
    /// 弹幕可使用的画面高度比例.
    pub display_area: f64,
}

impl Default for AssSettings {
    /// 返回可兼容现有 `render-ass` 行为的默认设置.
    fn default() -> Self {
        Self {
            font: "Microsoft YaHei".to_owned(),
            font_size: 24.0,
            alpha: 0.7,
            bold: false,
            border: 1.0,
            offset: 0.0,
            scroll_duration: 10.0,
            display_area: 1.0,
        }
    }
}

/// 一条已从数据库或其他来源解析的 Dandanplay 弹幕.
#[derive(Debug, Clone)]
pub struct AssDanmaku {
    /// 弹幕出现时间, 单位为秒.
    pub time: f64,
    /// 弹幕正文.
    pub content: String,
    /// Dandanplay 弹幕模式, 1 为滚动, 4 为底部, 5 为顶部.
    pub mode: i32,
    /// RGB 颜色整数.
    pub color: i32,
}

/// 内部用于轨道分配的弹幕数据.
#[derive(Debug, Clone)]
struct Danmaku {
    /// 加入偏移后的出现时间.
    time: f64,
    /// 弹幕正文.
    content: String,
    /// Dandanplay 弹幕模式.
    mode: i32,
    /// RGB 颜色整数.
    color: i32,
}

/// ASS 中弹幕的位置类别.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scroll,
    Top,
    Bottom,
}

/// 将 Dandanplay 缓存 JSON 解析为 ASS 字幕文本.
pub fn render_from_json(body: &str, settings: &AssSettings) -> anyhow::Result<String> {
    let value: Value = serde_json::from_str(body).context("弹幕缓存不是合法 JSON")?;

    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("JSON 中不存在 comments 数组"))?;

    let mut items = Vec::with_capacity(comments.len());

    for comment in comments {
        let p = comment.get("p").and_then(Value::as_str).unwrap_or("");
        let content = comment.get("m").and_then(Value::as_str).unwrap_or("");

        if p.is_empty() || content.is_empty() {
            continue;
        }

        let parts: Vec<&str> = p.split(',').collect();
        if parts.len() < 3 {
            continue;
        }

        let raw_time = parts[0].parse::<f64>().unwrap_or(0.0);
        let mode = parts[1].parse::<i32>().unwrap_or(1);
        let color = parts[2].parse::<i32>().unwrap_or(0xFF_FF_FF);

        items.push(AssDanmaku {
            time: raw_time,
            content: content.to_owned(),
            mode,
            color: color & 0x00FF_FFFF,
        });
    }

    render_danmaku(&items, settings)
}

/// 将已解析弹幕按设置渲染为 ASS 字幕文本.
pub fn render_danmaku(items: &[AssDanmaku], settings: &AssSettings) -> anyhow::Result<String> {
    let ass_font_size = resolve_font_size(settings.font_size);
    let lane_height = ((ass_font_size * 1.3).round() as i32).clamp(1, PLAY_RES_Y);
    let display_area = if settings.display_area.is_finite() {
        settings.display_area.clamp(0.01, 1.0)
    } else {
        1.0
    };
    let lane_count = (((PLAY_RES_Y as f64 * display_area) / lane_height as f64).floor() as i32)
        .clamp(1, 64) as usize;
    let duration = if settings.scroll_duration.is_finite() && settings.scroll_duration > 0.0 {
        settings.scroll_duration
    } else {
        10.0
    };
    let mut items = items.iter()
        .filter_map(|item| {
            let time = item.time + settings.offset;
            (time.is_finite() && time >= 0.0 && !item.content.is_empty()).then(|| Danmaku {
                time,
                content: item.content.clone(),
                mode: item.mode,
                color: item.color & 0x00FF_FFFF,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(Ordering::Equal));

    let mut output = String::new();
    write_header(&mut output, ass_font_size, settings);

    output.push_str("[Events]\n");
    output.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );

    let mut scroll_lanes = vec![None; lane_count];
    let mut top_lanes = vec![None; lane_count];
    let mut bottom_lanes = vec![None; lane_count];

    for item in items {
        let width = estimate_width(&item.content, ass_font_size);
        let kind = kind_from_mode(item.mode);

        let alpha = alpha_hex(settings.alpha);
        let color = color_override(item.color);
        let outline = outline_color_override(item.color);
        let text = escape_ass_text(&item.content);

        match kind {
            Kind::Scroll => {
                let Some(lane) = pick_scroll_lane(&mut scroll_lanes, item.time, width, duration)
                else {
                    continue;
                };

                let y = lane as f64 * lane_height as f64 + lane_height as f64 / 2.0;

                let line = format!(
                    "{{\\an4\\move({},{:.1},{:.1},{:.1}){}{}\\alpha{}}}{}",
                    PLAY_RES_X, y, -width, y, color, outline, alpha, text,
                );

                write_dialogue(
                    &mut output,
                    0,
                    item.time,
                    item.time + duration,
                    "Danmaku",
                    &line,
                )?;
            }

            Kind::Top => {
                let lane = pick_fixed_lane(&mut top_lanes, item.time, FIXED_DURATION);
                let y = lane as f64 * lane_height as f64 + 2.0;

                let line = format!(
                    "{{\\an8\\pos({},{:.1}){}{}\\alpha{}}}{}",
                    PLAY_RES_X / 2,
                    y,
                    color,
                    outline,
                    alpha,
                    text,
                );

                write_dialogue(
                    &mut output,
                    1,
                    item.time,
                    item.time + FIXED_DURATION,
                    "DanmakuTop",
                    &line,
                )?;
            }

            Kind::Bottom => {
                let lane = pick_fixed_lane(&mut bottom_lanes, item.time, FIXED_DURATION);
                let y = PLAY_RES_Y as f64 - lane as f64 * lane_height as f64 - 2.0;

                let line = format!(
                    "{{\\an2\\pos({},{:.1}){}{}\\alpha{}}}{}",
                    PLAY_RES_X / 2,
                    y,
                    color,
                    outline,
                    alpha,
                    text,
                );

                write_dialogue(
                    &mut output,
                    2,
                    item.time,
                    item.time + FIXED_DURATION,
                    "DanmakuBottom",
                    &line,
                )?;
            }
        }
    }

    Ok(output)
}

/// 将用户侧字号乘以 1.6 并限制到 ASS 空间 18..96.
fn resolve_font_size(input: f64) -> f64 {
    (input * 1.6).clamp(18.0, 96.0)
}

/// 将 Dandanplay 模式映射为 ASS 弹幕位置类别.
fn kind_from_mode(mode: i32) -> Kind {
    match mode {
        4 => Kind::Bottom,
        5 => Kind::Top,
        _ => Kind::Scroll,
    }
}

/// 按字符宽度估算弹幕占用的水平空间.
fn estimate_width(text: &str, font_size: f64) -> f64 {
    if text.is_empty() {
        return font_size;
    }

    text.encode_utf16().fold(0.0, |width, code| {
        let wide = code >= 0x1100
            && (code <= 0x115f
                || (0x2e80..=0xa4cf).contains(&code)
                || (0xac00..=0xd7a3).contains(&code)
                || (0xf900..=0xfaff).contains(&code)
                || (0xfe30..=0xfe4f).contains(&code)
                || (0xff00..=0xff60).contains(&code)
                || (0xffe0..=0xffe6).contains(&code));

        width + if wide { font_size } else { font_size * 0.6 }
    })
}

/// 为滚动弹幕选择一个不会立即碰撞的轨道.
fn pick_scroll_lane(
    lanes: &mut [Option<f64>],
    time: f64,
    width: f64,
    duration: f64,
) -> Option<usize> {
    for (index, free_at) in lanes.iter_mut().enumerate() {
        if free_at.is_none_or(|value| time >= value) {
            let velocity = (PLAY_RES_X as f64 + width) / duration;
            *free_at = Some(if velocity <= 0.0 {
                time + duration
            } else {
                time + width / velocity
            });
            return Some(index);
        }
    }

    None
}

/// 为固定位置弹幕选择一个轨道.
fn pick_fixed_lane(lanes: &mut [Option<f64>], time: f64, duration: f64) -> usize {
    let mut earliest = 0usize;
    let mut earliest_free_at = f64::INFINITY;

    for (index, free_at) in lanes.iter_mut().enumerate() {
        if free_at.is_none_or(|value| time >= value) {
            *free_at = Some(time + duration);
            return index;
        }

        if let Some(value) = *free_at {
            if value < earliest_free_at {
                earliest = index;
                earliest_free_at = value;
            }
        }
    }

    lanes[earliest] = Some(earliest_free_at + duration);
    earliest
}

/// 将不透明度编码为 ASS alpha 标签.
fn alpha_hex(opacity: f64) -> String {
    let alpha = ((1.0 - opacity.clamp(0.0, 1.0)) * 255.0).round() as i32;
    format!("&H{:02X}&", alpha.clamp(0, 255))
}

/// 将 RGB 颜色转换为 ASS BGR 颜色覆盖标签.
fn color_override(rgb: i32) -> String {
    let rgb = rgb & 0x00FF_FFFF;

    if rgb == 0x00FF_FFFF {
        return String::new();
    }

    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;

    format!("\\c&H{:06X}&", (b << 16) | (g << 8) | r)
}

/// 为接近黑色的文字选择白色描边.
fn outline_color_override(rgb: i32) -> &'static str {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;

    if r <= 8 && g <= 8 && b <= 8 {
        "\\3c&HFFFFFF&"
    } else {
        ""
    }
}

/// 转义 ASS 控制字符, 防止弹幕文本改变字幕结构.
fn escape_ass_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('\n', "\\N")
        .replace('\r', "")
}

/// 将秒数格式化为 ASS 小时, 分钟, 秒和百分之一秒.
fn format_ass_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let total_cs = (seconds * 100.0).round() as i64;
    let cs = total_cs % 100;
    let total_seconds = total_cs / 100;
    let second = total_seconds % 60;
    let minute = (total_seconds / 60) % 60;
    let hour = total_seconds / 3600;

    format!("{}:{:02}:{:02}.{:02}", hour, minute, second, cs)
}

/// 向 ASS Events 区域追加一条 Dialogue.
fn write_dialogue(
    output: &mut String,
    layer: i32,
    start: f64,
    end: f64,
    style: &str,
    text: &str,
) -> anyhow::Result<()> {
    writeln!(
        output,
        "Dialogue: {},{},{},{},,0,0,0,,{}",
        layer,
        format_ass_time(start),
        format_ass_time(end),
        style,
        text
    )
    .context("生成 ASS Dialogue 失败")
}

/// 写入 ASS Script Info 和弹幕样式头部.
fn write_header(output: &mut String, font_size: f64, settings: &AssSettings) {
    let font = if settings.font.trim().is_empty() {
        "Microsoft YaHei"
    } else {
        settings.font.trim()
    };

    let outline = settings.border.clamp(0.0, 8.0);

    writeln!(output, "[Script Info]").unwrap();
    writeln!(output, "ScriptType: v4.00+").unwrap();
    writeln!(output, "PlayResX: {PLAY_RES_X}").unwrap();
    writeln!(output, "PlayResY: {PLAY_RES_Y}").unwrap();
    writeln!(output, "Aspect Ratio: 16:9").unwrap();
    writeln!(output, "WrapStyle: 2").unwrap();
    writeln!(output, "ScaledBorderAndShadow: yes").unwrap();
    writeln!(output, "YCbCr Matrix: TV.709").unwrap();
    writeln!(output).unwrap();

    writeln!(output, "[V4+ Styles]").unwrap();
    writeln!(
        output,
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding"
    )
    .unwrap();

    let bold = if settings.bold { 1 } else { 0 };
    let size = format!("{font_size:.1}");

    for (name, alignment) in [("Danmaku", 2), ("DanmakuTop", 8), ("DanmakuBottom", 2)] {
        writeln!(
            output,
            "Style: {name},{font},{size},&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,{bold},0,0,0,100,100,0,0,1,{outline:.1},0,{alignment},0,0,0,1"
        )
        .unwrap();
    }

    writeln!(output).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dandanplay_comment_and_emits_ass() {
        let json = r#"{
            "count": 3,
            "comments": [
                {"cid": 1, "p": "1.23,1,16711680,100", "m": "hello"},
                {"cid": 2, "p": "2.00,5,65280,101", "m": "top"},
                {"cid": 3, "p": "3.00,4,255,102", "m": "bottom"}
            ]
        }"#;

        let ass = render_from_json(json, &AssSettings::default()).unwrap();

        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("PlayResX: 1920"));
        assert!(ass.contains("PlayResY: 1080"));
        assert!(ass.contains("Dialogue: 0,0:00:01.23,0:00:11.23,Danmaku"));
        assert!(ass.contains("Dialogue: 1,0:00:02.00,0:00:07.00,DanmakuTop"));
        assert!(ass.contains("Dialogue: 2,0:00:03.00,0:00:08.00,DanmakuBottom"));
        assert!(ass.contains("\\c&H0000FF&"));
    }

    #[test]
    fn escapes_ass_text_and_applies_offset() {
        let json = r#"{
            "comments": [
                {"p": "1.00,1,16777215,100", "m": "a{b}\nc"}
            ]
        }"#;

        let mut settings = AssSettings::default();
        settings.offset = -0.5;

        let ass = render_from_json(json, &settings).unwrap();

        assert!(ass.contains("Dialogue: 0,0:00:00.50,0:00:10.50"));
        assert!(ass.contains("a\\{b\\}\\Nc"));
    }

    #[test]
    fn bold_and_border_are_encoded_in_styles() {
        let json = r#"{"comments":[]}"#;
        let mut settings = AssSettings::default();
        settings.bold = true;
        settings.border = 2.0;

        let ass = render_from_json(json, &settings).unwrap();

        assert!(ass.contains(",1,0,0,0,100,100,0,0,1,2.0,0,2,"));
        assert!(ass.contains(",1,0,0,0,100,100,0,0,1,2.0,0,8,"));
    }
}
