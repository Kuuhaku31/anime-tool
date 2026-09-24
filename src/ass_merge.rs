use std::fmt::Write;

use anyhow::{Context, bail};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolution {
    width: f64,
    height: f64,
}

pub fn merge(subtitle: &str, danmaku: &str) -> anyhow::Result<String> {
    let subtitle_resolution = resolution(subtitle).context("字幕 ASS 缺少有效画幅")?;
    let danmaku_resolution = resolution(danmaku).context("弹幕 ASS 缺少有效画幅")?;
    let scale = (danmaku_resolution.width / subtitle_resolution.width)
        .min(danmaku_resolution.height / subtitle_resolution.height);
    let offset_x = (danmaku_resolution.width - subtitle_resolution.width * scale) / 2.0;
    let offset_y = (danmaku_resolution.height - subtitle_resolution.height * scale) / 2.0;

    let subtitle_styles = section(subtitle, "V4+ Styles")
        .or_else(|| section(subtitle, "V4 Styles"))
        .context("字幕 ASS 缺少样式区段")?;
    let danmaku_styles = section(danmaku, "V4+ Styles")
        .or_else(|| section(danmaku, "V4 Styles"))
        .context("弹幕 ASS 缺少样式区段")?;
    let subtitle_events = section(subtitle, "Events").context("字幕 ASS 缺少事件区段")?;
    let danmaku_events = section(danmaku, "Events").context("弹幕 ASS 缺少事件区段")?;

    let (subtitle_style_format, subtitle_style_rows) = parse_rows(subtitle_styles, "Style")?;
    let (danmaku_style_format, danmaku_style_rows) = parse_rows(danmaku_styles, "Style")?;
    ensure_same_format(&subtitle_style_format, &danmaku_style_format, "样式")?;

    let (subtitle_event_format, subtitle_event_rows) = parse_rows(subtitle_events, "Dialogue")?;
    let (danmaku_event_format, danmaku_event_rows) = parse_rows(danmaku_events, "Dialogue")?;
    ensure_same_format(&subtitle_event_format, &danmaku_event_format, "事件")?;

    let mut output = rewrite_script_info(subtitle, danmaku_resolution)?;
    output.push_str("\n[V4+ Styles]\nFormat: ");
    output.push_str(&subtitle_style_format.join(", "));
    output.push('\n');

    for mut row in subtitle_style_rows {
        scale_style(&subtitle_style_format, &mut row, scale, offset_x, offset_y)?;
        writeln!(output, "Style: {}", row.join(",")).unwrap();
    }
    for row in danmaku_style_rows {
        writeln!(output, "Style: {}", row.join(",")).unwrap();
    }

    output.push_str("\n[Events]\nFormat: ");
    output.push_str(&subtitle_event_format.join(", "));
    output.push('\n');
    for mut row in subtitle_event_rows {
        scale_event_text(&subtitle_event_format, &mut row, scale, offset_x, offset_y);
        writeln!(output, "Dialogue: {}", row.join(",")).unwrap();
    }
    for row in danmaku_event_rows {
        writeln!(output, "Dialogue: {}", row.join(",")).unwrap();
    }

    Ok(output)
}

fn resolution(input: &str) -> Option<Resolution> {
    let info = section(input, "Script Info")?;
    let width = property(info, "PlayResX")?.parse().ok()?;
    let height = property(info, "PlayResY")?.parse().ok()?;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(Resolution { width, height })
}

fn section<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("[{name}]");
    let start = input.find(&marker)? + marker.len();
    let rest = &input[start..];
    let end = rest
        .find("\n[")
        .or_else(|| rest.find("\r\n["))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn property<'a>(section: &'a str, name: &str) -> Option<&'a str> {
    section.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn parse_rows(section: &str, kind: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    let format = property(section, "Format")
        .with_context(|| format!("ASS {kind} 区段缺少 Format"))?
        .split(',')
        .map(|value| value.trim().to_owned())
        .collect::<Vec<_>>();
    let text_index = format
        .iter()
        .position(|field| field.eq_ignore_ascii_case("Text"));
    let mut rows = Vec::new();

    for line in section.lines() {
        let Some((prefix, value)) = line.split_once(':') else {
            continue;
        };
        if !prefix.trim().eq_ignore_ascii_case(kind) {
            continue;
        }
        let columns = if text_index.is_some() {
            value
                .trim_start()
                .splitn(format.len(), ',')
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        } else {
            value
                .trim_start()
                .split(',')
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        };
        if columns.len() != format.len() {
            bail!("ASS {kind} 字段数量与 Format 不一致");
        }
        rows.push(columns);
    }
    Ok((format, rows))
}

fn ensure_same_format(left: &[String], right: &[String], kind: &str) -> anyhow::Result<()> {
    let same = left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
    if !same {
        bail!("字幕和弹幕 ASS 的{kind} Format 不一致");
    }
    Ok(())
}

fn rewrite_script_info(input: &str, target: Resolution) -> anyhow::Result<String> {
    let info = section(input, "Script Info").context("字幕 ASS 缺少 Script Info 区段")?;
    let mut output = String::from("[Script Info]\n");
    for line in info.lines() {
        if let Some((key, _)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("PlayResX") {
                writeln!(output, "PlayResX: {}", format_number(target.width)).unwrap();
                continue;
            }
            if key.trim().eq_ignore_ascii_case("PlayResY") {
                writeln!(output, "PlayResY: {}", format_number(target.height)).unwrap();
                continue;
            }
        }
        if !line.trim().is_empty() {
            writeln!(output, "{line}").unwrap();
        }
    }
    Ok(output)
}

fn scale_style(
    format: &[String],
    row: &mut [String],
    scale: f64,
    offset_x: f64,
    offset_y: f64,
) -> anyhow::Result<()> {
    for (name, value) in format.iter().zip(row.iter_mut()) {
        let factor = if name.eq_ignore_ascii_case("MarginL") || name.eq_ignore_ascii_case("MarginR")
        {
            Some((scale, offset_x))
        } else if name.eq_ignore_ascii_case("MarginV") {
            Some((scale, offset_y))
        } else if name.eq_ignore_ascii_case("Fontsize")
            || name.eq_ignore_ascii_case("Outline")
            || name.eq_ignore_ascii_case("Shadow")
            || name.eq_ignore_ascii_case("Spacing")
        {
            Some((scale, 0.0))
        } else {
            None
        };
        if let Some((factor, offset)) = factor {
            let number = value
                .trim()
                .parse::<f64>()
                .with_context(|| format!("无效的样式数值 {name}: {value}"))?;
            *value = format_number(number * factor + offset);
        }
    }
    Ok(())
}

fn scale_event_text(
    format: &[String],
    row: &mut [String],
    scale: f64,
    offset_x: f64,
    offset_y: f64,
) {
    for (name, value) in format.iter().zip(row.iter_mut()) {
        let offset = if name.eq_ignore_ascii_case("MarginL") || name.eq_ignore_ascii_case("MarginR")
        {
            Some(offset_x)
        } else if name.eq_ignore_ascii_case("MarginV") {
            Some(offset_y)
        } else {
            None
        };
        if let Some(offset) = offset {
            if let Ok(number) = value.trim().parse::<f64>() {
                if number != 0.0 {
                    *value = format_number(number * scale + offset);
                }
            }
        }
    }
    let Some(index) = format
        .iter()
        .position(|field| field.eq_ignore_ascii_case("Text"))
    else {
        return;
    };
    row[index] = transform_override_coordinates(&row[index], scale, offset_x, offset_y);
}

fn transform_override_coordinates(text: &str, scale: f64, offset_x: f64, offset_y: f64) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('\\') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let tags = ["\\pos(", "\\org(", "\\move(", "\\clip(", "\\iclip("];
        let Some(tag) = tags.iter().find(|tag| rest.starts_with(**tag)) else {
            output.push('\\');
            rest = &rest[1..];
            continue;
        };
        let Some(end) = rest.find(')') else {
            output.push_str(rest);
            return output;
        };
        let values = &rest[tag.len()..end];
        let parts = values.split(',').map(str::trim).collect::<Vec<_>>();
        let coordinate_count = if *tag == "\\move(" { 4 } else { 2 };
        let is_rect_clip = (*tag == "\\clip(" || *tag == "\\iclip(") && parts.len() == 4;
        let count = if is_rect_clip { 4 } else { coordinate_count };
        if parts.len() >= count
            && parts[..count]
                .iter()
                .all(|value| value.parse::<f64>().is_ok())
        {
            output.push_str(tag);
            for (index, value) in parts.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                if index < count {
                    let number = value.parse::<f64>().unwrap();
                    let transformed = if index % 2 == 0 {
                        number * scale + offset_x
                    } else {
                        number * scale + offset_y
                    };
                    output.push_str(&format_number(transformed));
                } else {
                    output.push_str(value);
                }
            }
            output.push(')');
        } else {
            output.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

fn format_number(value: f64) -> String {
    if (value - value.round()).abs() < 0.000_001 {
        format!("{value:.0}")
    } else {
        let value = format!("{value:.4}");
        value.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE_FORMAT: &str = "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding";
    const EVENT_FORMAT: &str =
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

    fn ass(width: i32, height: i32, style: &str, dialogue: &str) -> String {
        format!(
            "[Script Info]\nPlayResX: {width}\nPlayResY: {height}\n\n[V4+ Styles]\n{STYLE_FORMAT}\n{style}\n\n[Events]\n{EVENT_FORMAT}\n{dialogue}\n"
        )
    }

    #[test]
    fn scales_equal_aspect_ratio_and_appends_danmaku() {
        let subtitle = ass(
            1280,
            720,
            "Style: Default,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,20,1",
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\pos(100,200)}sub",
        );
        let danmaku = ass(
            1920,
            1080,
            "Style: Danmaku,Arial,30,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,0,0,0,1",
            "Dialogue: 1,0:00:00.00,0:00:01.00,Danmaku,,0,0,0,,danmaku",
        );

        let merged = merge(&subtitle, &danmaku).unwrap();

        assert!(merged.contains("Style: Default,Arial,30,"));
        assert!(merged.contains("{\\pos(150,300)}sub"));
        assert!(merged.contains("Dialogue: 1,0:00:00.00,0:00:01.00,Danmaku"));
    }

    #[test]
    fn expands_subtitle_canvas_and_centers_content() {
        let subtitle = ass(
            640,
            480,
            "Style: Default,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,20,1",
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\move(0,0,640,480)}sub",
        );
        let danmaku = ass(
            1920,
            1080,
            "Style: Danmaku,Arial,30,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,0,0,0,1",
            "Dialogue: 1,0:00:00.00,0:00:01.00,Danmaku,,0,0,0,,danmaku",
        );

        let merged = merge(&subtitle, &danmaku).unwrap();

        assert!(merged.contains("Style: Default,Arial,45,"));
        assert!(merged.contains(",262.5,262.5,45,1"));
        assert!(merged.contains("Default,,0,0,0,,"));
        assert!(merged.contains("{\\move(240,0,1680,1080)}sub"));
    }
}
