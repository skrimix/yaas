/// Takes `dumpsys battery` output and converts raw values to human-readable format
pub fn humanize_dump(input: &str) -> String {
    fn fmt(n: f64) -> String {
        if (n.fract()).abs() < 1e-9 {
            format!("{:.0}", n)
        } else if n.abs() < 10.0 {
            format!("{:.1}", n)
        } else {
            format!("{:.2}", n)
        }
    }

    fn status_label(code: i64) -> &'static str {
        match code {
            1 => "Unknown",
            2 => "Charging",
            3 => "Discharging",
            4 => "Not charging",
            5 => "Full",
            _ => "",
        }
    }

    fn health_label(code: i64) -> &'static str {
        match code {
            1 => "Unknown",
            2 => "Good",
            3 => "Overheat",
            4 => "Dead",
            5 => "Over voltage",
            6 => "Failure",
            7 => "Cold",
            _ => "",
        }
    }

    let mut out = String::with_capacity(input.len() + 128);
    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');
        // Preserve leading indentation
        let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
        let (indent, content) = line.split_at(indent_len);

        let mut parts = content.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let value_raw = parts.next().map(|s| s.trim()).unwrap_or("");

        if key.is_empty() || !content.contains(':') {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let key_lc = key.to_ascii_lowercase();
        let mut formatted = None::<String>;

        let int_val = value_raw.parse::<i64>().ok();
        match key_lc.as_str() {
            "max charging current" => {
                if let Some(v) = int_val {
                    // µA -> A
                    let amps = (v as f64) / 1e6;
                    formatted = Some(format!("{}: {} µA ({} A)", key, value_raw, fmt(amps)));
                }
            }
            "max charging voltage" => {
                if let Some(v) = int_val {
                    // µV -> V
                    let volts = (v as f64) / 1e6;
                    formatted = Some(format!("{}: {} µV ({} V)", key, value_raw, fmt(volts)));
                }
            }
            "charge counter" => {
                if let Some(v) = int_val {
                    // µAh -> mAh
                    let mah = (v as f64) / 1000.0;
                    formatted = Some(format!("{}: {} µAh ({} mAh)", key, value_raw, fmt(mah)));
                }
            }
            "voltage" => {
                if let Some(v) = int_val {
                    // mV -> V (common for dumpsys)
                    let volts = (v as f64) / 1000.0;
                    formatted = Some(format!("{}: {} mV ({} V)", key, value_raw, fmt(volts)));
                }
            }
            "temperature" => {
                if let Some(v) = int_val {
                    // tenths of °C -> °C
                    let c = (v as f64) / 10.0;
                    formatted = Some(format!("{}: {} ({} °C)", key, value_raw, fmt(c)));
                }
            }
            "status" => {
                if let Some(v) = int_val {
                    let label = status_label(v);
                    if !label.is_empty() {
                        formatted = Some(format!("{}: {} ({})", key, value_raw, label));
                    }
                }
            }
            "health" => {
                if let Some(v) = int_val {
                    let label = health_label(v);
                    if !label.is_empty() {
                        formatted = Some(format!("{}: {} ({})", key, value_raw, label));
                    }
                }
            }
            _ => {}
        }

        if let Some(f) = formatted {
            out.push_str(indent);
            out.push_str(&f);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}
