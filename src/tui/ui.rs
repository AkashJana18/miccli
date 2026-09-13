use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Paragraph, Row, Sparkline, Table, Tabs, Wrap,
    },
    Frame,
};

use super::waveform::{waveform_to_blocks, WaveformHistory};
use super::AppMode;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Live,
    Models,
    Config,
    Help,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[Tab::Live, Tab::Models, Tab::Config, Tab::Help]
    }
    pub fn title(self) -> &'static str {
        match self {
            Tab::Live => "  ◉ Live  ",
            Tab::Models => "  ◈ Models  ",
            Tab::Config => "  ⚙ Config  ",
            Tab::Help => "  ? Help  ",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Tab::Live => 0,
            Tab::Models => 1,
            Tab::Config => 2,
            Tab::Help => 3,
        }
    }
    pub fn from_index(i: usize) -> Self {
        match i % 4 {
            0 => Tab::Live,
            1 => Tab::Models,
            2 => Tab::Config,
            _ => Tab::Help,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LatencyStats {
    pub transcribe: std::time::Duration,
    pub cleanup: std::time::Duration,
    pub insert: std::time::Duration,
    pub total: std::time::Duration,
}

#[derive(Clone, Debug)]
pub struct ModelRow {
    pub name: &'static str,
    pub size: &'static str,
    pub installed: bool,
    pub path: String,
}

#[derive(Debug)]
pub struct AppState {
    pub is_recording: bool,
    pub hotkey: String,
    pub app_name: Option<String>,
    pub strategy: String,
    pub transcription: String,
    pub raw_text: String,
    pub status: String,
    pub latencies: Option<LatencyStats>,
    pub vad_status: String,
    pub error: Option<String>,
    pub tab: Tab,
    pub tick: usize,
    pub recording_samples: usize,
    pub model_name: String,
    pub model_installed: bool,
    pub vad_threshold: f32,
    pub models: Vec<ModelRow>,
    pub config_text: String,
    pub config_path: String,
    pub version: String,
    pub mode: AppMode,
    pub paused: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            is_recording: false,
            hotkey: "Shift+Control".to_string(),
            app_name: None,
            strategy: "auto".to_string(),
            transcription: String::new(),
            raw_text: String::new(),
            status: "Hold hotkey to talk — release to transcribe".to_string(),
            latencies: None,
            vad_status: "idle".to_string(),
            error: None,
            tab: Tab::Live,
            tick: 0,
            recording_samples: 0,
            model_name: "small".to_string(),
            model_installed: false,
            vad_threshold: 0.5,
            models: vec![],
            config_text: String::new(),
            config_path: "~/.config/miccli/config.toml".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            mode: AppMode::Overlay,
            paused: false,
        }
    }
}

impl AppState {
    pub fn next_tab(&mut self) {
        self.tab = Tab::from_index((self.tab.index() + 1) % 4);
    }
    pub fn prev_tab(&mut self) {
        self.tab = Tab::from_index((self.tab.index() + 3) % 4);
    }
    pub fn set_tab(&mut self, idx: usize) {
        self.tab = Tab::from_index(idx);
    }
}

#[allow(dead_code)]
pub fn render(frame: &mut Frame, app: &AppState, waveform: &WaveformHistory) {
    match app.mode {
        AppMode::Overlay => render_overlay(frame, app, waveform),
        _ => render_dashboard(frame, app, waveform),
    }
}

pub fn render_dashboard(frame: &mut Frame, app: &AppState, waveform: &WaveformHistory) {
    let area = frame.area();

    // Outer layout: header, tabs, body, footer
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(3), // tabs
            Constraint::Min(10),   // body
            Constraint::Length(1), // footer help
        ])
        .split(area);

    render_header(frame, vertical[0], app);
    render_tabs(frame, vertical[1], app);
    match app.tab {
        Tab::Live => render_live(frame, vertical[2], app, waveform),
        Tab::Models => render_models(frame, vertical[2], app),
        Tab::Config => render_config(frame, vertical[2], app),
        Tab::Help => render_help(frame, vertical[2], app),
    }
    render_footer(frame, vertical[3], app);
}

/// Minimal whisperflow-style overlay: top-aligned small box (~6 lines), no tabs.
pub fn render_overlay(frame: &mut Frame, app: &AppState, waveform: &WaveformHistory) {
    let area = frame.area();
    // Top-aligned overlay: height 7 if we have transcription, else 6
    let has_text = !app.transcription.is_empty();
    let box_h: u16 = if has_text { 7 } else { 6 };
    let overlay_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: box_h.min(area.height),
    };

    let border_col = if app.paused {
        Color::Yellow
    } else if app.is_recording {
        Color::Red
    } else if app.error.is_some() {
        Color::Red
    } else {
        Color::Cyan
    };

    let title = if app.paused {
        " miccli — ⏸ paused (toggle to resume) "
    } else if app.is_recording {
        " miccli — ● recording "
    } else {
        " miccli "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .title(title)
        .title_style(Style::default().fg(border_col).add_modifier(Modifier::BOLD));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    if inner.height < 2 || inner.width < 10 {
        return;
    }

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header line
            Constraint::Length(1), // waveform
            Constraint::Min(1),    // transcription/status
            Constraint::Length(1), // footer hint
        ])
        .split(inner);

    // Header line: hotkey + app + strategy (+ paused)
    let header_line = if app.paused {
        Line::from(vec![
            Span::styled(" ⏸ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("paused", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}  ", app.hotkey), Style::default().fg(Color::DarkGray)),
            Span::styled("toggle to resume", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        ])
    } else {
        let dot = if app.is_recording {
            if app.tick % 10 < 5 { "●" } else { "○" }
        } else {
            "■"
        };
        let rec_style = if app.is_recording {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let app_name = app.app_name.as_deref().unwrap_or("—");
        Line::from(vec![
            Span::styled(format!(" {} {}  ", dot, if app.is_recording { "REC" } else { "IDLE" }), rec_style),
            Span::styled(format!("{}  ", app.hotkey), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{}  ", app_name), Style::default().fg(Color::DarkGray)),
            Span::styled(strategy_icon(&app.strategy), Style::default().fg(match app.strategy.as_str() { "type" => Color::Magenta, "paste" => Color::Green, _ => Color::DarkGray })),
        ])
    };
    frame.render_widget(Paragraph::new(header_line).alignment(Alignment::Left), inner_chunks[0]);

    // Waveform line (inline blocks)
    let data = waveform.data();
    let wf_width = inner_chunks[1].width as usize;
    let wf_str = if app.paused {
        "─ paused ─".to_string()
    } else if data.is_empty() && app.is_recording {
        "▁▂▃ listening…".to_string()
    } else if data.is_empty() {
        String::new()
    } else {
        waveform_to_blocks(&data, wf_width.saturating_sub(2))
    };
    let wf_color = if app.is_recording {
        let max = waveform.max_level();
        if max > 70 { Color::Red } else if max > 35 { Color::Yellow } else { Color::Cyan }
    } else {
        Color::DarkGray
    };
    let wf_line = Line::from(vec![Span::styled(
        format!(" {}", wf_str),
        Style::default().fg(wf_color),
    )]);
    frame.render_widget(Paragraph::new(wf_line), inner_chunks[1]);

    // Transcription / status
    let mid_para = if let Some(err) = &app.error {
        Paragraph::new(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Red)),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]))
        .wrap(Wrap { trim: true })
    } else if !app.transcription.is_empty() {
        let mut lines = vec![Line::from(vec![Span::styled(
            format!("\"{}\"", app.transcription),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )])];
        if !app.raw_text.is_empty() && app.raw_text != app.transcription {
            lines.push(Line::from(vec![
                Span::styled("raw: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                Span::styled(format!("\"{}\"", app.raw_text), Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC)),
            ]));
        }
        Paragraph::new(lines).wrap(Wrap { trim: true })
    } else if app.is_recording {
        Paragraph::new(Line::from(vec![Span::styled(
            "  listening… speak now",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]))
    } else if !app.status.is_empty() {
        Paragraph::new(Line::from(vec![Span::styled(
            app.status.clone(),
            Style::default().fg(Color::DarkGray),
        )]))
        .wrap(Wrap { trim: true })
    } else {
        Paragraph::new(Line::from(""))
    };
    frame.render_widget(mid_para, inner_chunks[2]);

    // Footer hint (only when not paused? always show toggle hint)
    let footer = if app.is_recording {
        Line::from(vec![Span::styled(
            "  hold to talk — release to transcribe",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )])
    } else {
        Line::from(vec![
            Span::styled(" toggle", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" pause  ", Style::default().fg(Color::DarkGray)),
        ])
    };
    frame.render_widget(Paragraph::new(footer), inner_chunks[3]);
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let recording_style = if app.is_recording {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Pulse dot when recording (tick toggles)
    let dot = if app.is_recording {
        if app.tick % 10 < 5 { "●" } else { "○" }
    } else {
        "■"
    };
    let rec_label = if app.is_recording { "REC" } else { "IDLE" };

    let left = Line::from(vec![
        Span::styled("  ◉ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            "miccli",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  v{}  ", app.version),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{} {}", dot, rec_label),
            recording_style,
        ),
        Span::styled(
            format!("  {}  ", app.hotkey),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let right_text = if let Some(name) = &app.app_name {
        format!("{}  {} ", strategy_icon(&app.strategy), name)
    } else {
        "auto  ".to_string()
    };

    let strategy_color = match app.strategy.as_str() {
        "type" => Color::Magenta,
        "paste" => Color::Green,
        _ => Color::DarkGray,
    };

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.is_recording {
            Color::Red
        } else {
            Color::DarkGray
        }))
        .title(Line::from(vec![
            Span::styled(" miccli — voice dictation ", Style::default().fg(Color::Cyan)),
        ]))
        .title_alignment(Alignment::Center);

    let inner = header_block.inner(area);
    frame.render_widget(header_block, area);

    let header_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(34)])
        .split(inner);

    let left_para = Paragraph::new(left).alignment(Alignment::Left);
    frame.render_widget(left_para, header_layout[0]);

    let strat_line = Line::from(vec![
        Span::styled(
            right_text,
            Style::default().fg(strategy_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if app.model_installed { "● model ready" } else { "○ model missing" },
            Style::default().fg(if app.model_installed { Color::Green } else { Color::Red }),
        ),
    ]);
    let right_para = Paragraph::new(strat_line).alignment(Alignment::Right);
    frame.render_widget(right_para, header_layout[1]);
}

fn strategy_icon(s: &str) -> &'static str {
    match s {
        "type" => "⌨ type",
        "paste" => "⎘ paste",
        "clipboard_only" => "⧉ clip",
        _ => "◐ auto",
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, app: &AppState) {
    let titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .select(app.tab.index())
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::from_u32(0x1a1a2e))
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
    frame.render_widget(tabs, area);
}

fn render_live(frame: &mut Frame, area: Rect, app: &AppState, waveform: &WaveformHistory) {
    // Live tab: waveform + transcription + stats + status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // waveform
            Constraint::Min(6),    // transcription
            Constraint::Length(7), // stats + status
        ])
        .split(area);

    render_waveform(frame, chunks[0], app, waveform);
    render_transcription(frame, chunks[1], app);
    render_stats(frame, chunks[2], app);
}

fn render_waveform(frame: &mut Frame, area: Rect, app: &AppState, waveform: &WaveformHistory) {
    let data = waveform.data();
    let max = waveform.max_level();
    let title = if app.is_recording {
        format!(
            " waveform — ● recording  {} samples  peak {}% ",
            app.recording_samples, max
        )
    } else if data.is_empty() {
        " waveform — hold hotkey to see live audio ".to_string()
    } else {
        format!(" waveform — ■ idle  last peak {}% ", max)
    };

    let color = if app.is_recording {
        if max > 70 {
            Color::Red
        } else if max > 35 {
            Color::Yellow
        } else {
            Color::Cyan
        }
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.is_recording { color } else { Color::DarkGray }))
        .title(title)
        .title_style(Style::default().fg(color).add_modifier(Modifier::BOLD));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    // Sparkline needs at least 1 row; leave border padding
    let spark_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height,
    };

    if data.is_empty() {
        let hint = Paragraph::new(Line::from(vec![Span::styled(
            "  ──  no audio yet  ──  hold Shift+Control and speak  ──",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]))
        .alignment(Alignment::Center);
        frame.render_widget(hint, spark_area);
        return;
    }

    // Ensure data fits width: if more points than width, take last width points
    let display_data: Vec<u64> = if data.len() > spark_area.width as usize {
        data[data.len() - spark_area.width as usize..].to_vec()
    } else {
        data.clone()
    };

    let sparkline = Sparkline::default()
        .block(Block::default())
        .data(&display_data)
        .max(100)
        .style(Style::default().fg(color));

    frame.render_widget(sparkline, spark_area);

    // Bottom axis labels
    if app.is_recording {
        let bar = "▁▂▃▄▅▆▇█".chars().collect::<Vec<_>>();
        // Show live level as blocks in corner instead of axis
        let level = *display_data.last().unwrap_or(&0);
        let idx = ((level as f32 / 100.0) * (bar.len() as f32 - 1.0)) as usize;
        let block_char = bar[idx.min(bar.len() - 1)];
        let lvl_span = Line::from(vec![
            Span::styled(
                format!(" {} {}% ", block_char, level),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", app.vad_status),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let lvl_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        let p = Paragraph::new(lvl_span).alignment(Alignment::Right);
        frame.render_widget(p, lvl_area);
    }
}

fn render_transcription(frame: &mut Frame, area: Rect, app: &AppState) {
    let has_text = !app.transcription.is_empty() || !app.raw_text.is_empty();

    let title = if app.is_recording {
        " transcription — listening… "
    } else if has_text {
        " transcription — last result "
    } else {
        " transcription "
    };

    let border_col = if app.is_recording {
        Color::Red
    } else if has_text {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .title(title)
        .title_style(Style::default().fg(border_col).add_modifier(Modifier::BOLD))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    if !has_text {
        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  No transcription yet.",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )]),
            Line::from(vec![Span::styled(
                "  Hold Shift+Control, speak, release — your words appear here and are typed into the focused app.",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Tip: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(
                    "Say “open curly brace” → {   •   “fat arrow” → =>   •   “double quote” → \"",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    // Show transcription with cursor-like styling; also show raw if different
    let mut lines: Vec<Line> = Vec::new();

    if !app.transcription.is_empty() {
        // Main cleaned text — big, white
        lines.push(Line::from(vec![Span::styled(
            format!("\"{}\"", app.transcription),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));
    }

    if !app.raw_text.is_empty() && app.raw_text != app.transcription {
        lines.push(Line::from(vec![
            Span::styled("raw: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            Span::styled(
                format!("\"{}\"", app.raw_text),
                Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC),
            ),
        ]));
        lines.push(Line::from(""));
    }

    if let Some(err) = &app.error {
        lines.push(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(Color::Red)),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
    }

    // Status line at bottom of transcription block
    let status_style = if app.is_recording {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    lines.push(Line::from(vec![Span::styled(
        app.status.clone(),
        status_style,
    )]));

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    frame.render_widget(para, inner);
}

fn render_stats(frame: &mut Frame, area: Rect, app: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: pipeline stats
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" pipeline ")
        .title_style(Style::default().fg(Color::DarkGray));

    let left_inner = left_block.inner(cols[0]);
    frame.render_widget(left_block, cols[0]);

    let stats_lines = if let Some(lat) = &app.latencies {
        vec![
            Line::from(vec![
                Span::styled("  transcribe  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:>6.0?}", lat.transcribe),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   cleanup  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:>6.0?}", lat.cleanup),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::styled("  insert      ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:>6.0?}", lat.insert),
                    Style::default().fg(Color::Green),
                ),
                Span::styled("   total    ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:>6.0?}", lat.total),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![Span::styled(
                "  No run yet — hold hotkey to transcribe",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )]),
            Line::from(vec![
                Span::styled("  model: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &app.model_name,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   VAD: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.2}", app.vad_threshold),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
        ]
    };

    frame.render_widget(
        Paragraph::new(stats_lines).alignment(Alignment::Left),
        left_inner,
    );

    // Right: app + insertion
    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" insertion ")
        .title_style(Style::default().fg(Color::DarkGray));

    let right_inner = right_block.inner(cols[1]);
    frame.render_widget(right_block, cols[1]);

    let app_line = if let Some(name) = &app.app_name {
        Line::from(vec![
            Span::styled("  app  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                name.clone(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![Span::styled(
            "  app  (detecting frontmost…)",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )])
    };

    let strat_line = Line::from(vec![
        Span::styled("  via  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}  ", app.strategy),
            Style::default().fg(match app.strategy.as_str() {
                "type" => Color::Magenta,
                "paste" => Color::Green,
                _ => Color::Yellow,
            }).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            match app.strategy.as_str() {
                "type" => "20ms/char • safe for TUIs",
                "paste" => "clipboard • fast",
                _ => "auto • detects app",
            },
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let right_lines = vec![app_line, strat_line];
    frame.render_widget(
        Paragraph::new(right_lines).alignment(Alignment::Left),
        right_inner,
    );
}

fn render_models(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" whisper models — stored in ~/.config/miccli/models/ ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    let header = Row::new(vec![
        Cell::from(Span::styled("  model", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("size", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("status", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("path", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .models
        .iter()
        .map(|m| {
            let is_active = m.name == app.model_name;
            let name_style = if is_active {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let status = if m.installed { "✅ downloaded" } else { "○ not downloaded" };
            let status_style = if m.installed {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let marker = if is_active { "▶ " } else { "  " };
            Row::new(vec![
                Cell::from(Span::styled(format!("{}{}", marker, m.name), name_style)),
                Cell::from(Span::styled(m.size, Style::default().fg(Color::DarkGray))),
                Cell::from(Span::styled(status, status_style)),
                Cell::from(Span::styled(m.path.clone(), Style::default().fg(Color::DarkGray))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(18),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(Color::from_u32(0x1a1a2e)))
        .column_spacing(1);

    frame.render_widget(table, inner);

    // Hint at bottom
    if inner.height > (app.models.len() as u16 + 3) {
        let hint_area = Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        };
        let hint = Paragraph::new(Line::from(vec![
            Span::styled("  ▶ active model  ", Style::default().fg(Color::Yellow)),
            Span::styled("•  ", Style::default().fg(Color::DarkGray)),
            Span::styled("miccli models download <tiny|base|small|medium>", Style::default().fg(Color::Cyan)),
        ]))
        .alignment(Alignment::Left);
        frame.render_widget(hint, hint_area);
    }
}

fn render_config(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(format!(" config — {} ", app.config_path))
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Show config text with syntax-ish coloring; also a small table of parsed values on top
    let config_lines: Vec<Line> = app
        .config_text
        .lines()
        .map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with('[') {
                Line::from(Span::styled(l.to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
            } else if trimmed.starts_with('#') || trimmed.is_empty() {
                Line::from(Span::styled(l.to_string(), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)))
            } else if trimmed.contains('=') {
                let parts: Vec<&str> = l.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(parts[0].to_string(), Style::default().fg(Color::Cyan)),
                        Span::styled("=", Style::default().fg(Color::White)),
                        Span::styled(parts[1].to_string(), Style::default().fg(Color::Yellow)),
                    ])
                } else {
                    Line::from(Span::styled(l.to_string(), Style::default().fg(Color::White)))
                }
            } else {
                Line::from(Span::styled(l.to_string(), Style::default().fg(Color::Gray)))
            }
        })
        .collect();

    // Top summary box inside? Keep it simple: just show config text
    let para = Paragraph::new(config_lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    frame.render_widget(para, inner);
}

fn render_help(frame: &mut Frame, area: Rect, _app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Green))
        .title(" help — keys & permissions ")
        .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let help_lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Keys",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![
            Span::styled("    Shift+Control  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("hold to record, release to transcribe & insert", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("    Tab / Shift+Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  next / prev tab", Style::default().fg(Color::White)),
            Span::styled("    1 2 3 4", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  jump to Live / Models / Config / Help", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("    q / Ctrl+C    ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("quit", Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Insertion",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![Span::styled(
            "    miccli detects the frontmost app and picks the right strategy:",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![
            Span::styled("      ⌨ type", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("  — terminals & Electron TUIs (opencode, Codex, Claude) — slow char typing avoids", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![Span::styled(
            "               “[Pasted text]” collapse",
            Style::default().fg(Color::Gray),
        )]),
        Line::from(vec![
            Span::styled("      ⎘ paste", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" — IDEs & editors (VS Code, IntelliJ, Sublime) — fast clipboard", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![Span::styled(
            "    Override per-app in ~/.config/miccli/config.toml:",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]),
        Line::from(vec![Span::styled(
            "      [[insertion.apps]]  bundle_id = \"dev.opencode\"  strategy = \"type\"",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Permissions (macOS)",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![
            Span::styled("    Microphone", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" — auto-prompted on first run", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("    Accessibility", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" — System Settings → Privacy & Security → Accessibility → allow miccli", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![Span::styled(
            "      Without it: hotkey won’t fire and text insertion will fail.",
            Style::default().fg(Color::Red).add_modifier(Modifier::ITALIC),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Tips",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(vec![Span::styled(
            "    • Say “open curly brace”, “close paren”, “fat arrow”, “semicolon” for code.",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![Span::styled(
            "    • Run `miccli --help` and `miccli config --path` for CLI options.",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![Span::styled(
            "    • Models live in ~/.config/miccli/models/ — small (466MB) is recommended.",
            Style::default().fg(Color::White),
        )]),
    ];

    let para = Paragraph::new(help_lines)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    frame.render_widget(para, inner);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &AppState) {
    let tab_hint = match app.tab {
        Tab::Live => "live: waveform + transcript",
        Tab::Models => "models: whisper download status",
        Tab::Config => "config: ~/.config/miccli/config.toml",
        Tab::Help => "help: keys & perms",
    };
    let line = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" quit  ", Style::default().fg(Color::DarkGray)),
        Span::styled("tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" switch  ", Style::default().fg(Color::DarkGray)),
        Span::styled("1-4", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" jump  ", Style::default().fg(Color::DarkGray)),
        Span::styled("⇧^ hold", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(" talk  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("│ {}", tab_hint), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        Span::styled(
            format!(" │ miccli v{}  •  {} ", app.version, if app.is_recording { "● REC" } else { "■ idle" }),
            Style::default().fg(if app.is_recording { Color::Red } else { Color::DarkGray }),
        ),
    ]);
    let para = Paragraph::new(line).alignment(Alignment::Center);
    frame.render_widget(para, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::WaveformHistory;
    use ratatui::{backend::TestBackend, Terminal};

    fn make_app(tab: Tab, recording: bool) -> AppState {
        let mut app = AppState::default();
        app.mode = crate::tui::AppMode::Dashboard;
        app.tab = tab;
        app.is_recording = recording;
        app.transcription = "Hello world from miccli".to_string();
        app.raw_text = "hello world from mick lee".to_string();
        app.hotkey = "Shift+Control".to_string();
        app.app_name = Some("dev.opencode".to_string());
        app.strategy = "type".to_string();
        app.model_name = "small".to_string();
        app.model_installed = true;
        app.models = vec![
            ModelRow {
                name: "tiny",
                size: "75 MB",
                installed: true,
                path: "/tmp/ggml-tiny.bin".to_string(),
            },
            ModelRow {
                name: "small",
                size: "466 MB",
                installed: true,
                path: "/tmp/ggml-small.bin".to_string(),
            },
        ];
        app.config_text = "[hotkey]\nmodifier = \"Shift+Control\"\n".to_string();
        app.latencies = Some(LatencyStats {
            transcribe: std::time::Duration::from_millis(820),
            cleanup: std::time::Duration::from_millis(120),
            insert: std::time::Duration::from_millis(30),
            total: std::time::Duration::from_millis(970),
        });
        app
    }

    fn make_overlay_app(recording: bool) -> AppState {
        let mut app = AppState::default();
        app.mode = crate::tui::AppMode::Overlay;
        app.is_recording = recording;
        app.transcription = "Hello overlay".to_string();
        app.hotkey = "Shift+Control".to_string();
        app.app_name = Some("dev.opencode".to_string());
        app.strategy = "type".to_string();
        app.tick = 3;
        app
    }

    fn render_to_string(app: &AppState, waveform: &WaveformHistory) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, app, waveform))
            .unwrap();
        // Convert buffer to string for snapshot-ish check
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_live_idle() {
        let app = make_app(Tab::Live, false);
        let mut wf = WaveformHistory::new(80);
        for i in 0..20 {
            wf.push_level((i * 3 % 40) as u64);
        }
        let s = render_to_string(&app, &wf);
        assert!(s.contains("miccli"));
        assert!(s.contains("transcription"));
        assert!(s.contains("waveform"));
    }

    #[test]
    fn renders_live_recording() {
        let app = make_app(Tab::Live, true);
        let mut wf = WaveformHistory::new(80);
        for _ in 0..30 {
            wf.push_level(60);
        }
        let s = render_to_string(&app, &wf);
        assert!(s.contains("REC") || s.contains("recording"));
    }

    #[test]
    fn renders_models_tab() {
        let app = make_app(Tab::Models, false);
        let wf = WaveformHistory::new(80);
        let s = render_to_string(&app, &wf);
        assert!(s.contains("whisper models") || s.contains("tiny"));
    }

    #[test]
    fn renders_config_tab() {
        let app = make_app(Tab::Config, false);
        let wf = WaveformHistory::new(80);
        let s = render_to_string(&app, &wf);
        assert!(s.contains("config") || s.contains("hotkey"));
    }

    #[test]
    fn renders_help_tab() {
        let app = make_app(Tab::Help, false);
        let wf = WaveformHistory::new(80);
        let s = render_to_string(&app, &wf);
        assert!(s.contains("Keys") || s.contains("Permissions"));
    }

    #[test]
    fn handles_narrow_terminal() {
        let app = make_app(Tab::Live, false);
        let wf = WaveformHistory::new(40);
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let res = terminal.draw(|f| render(f, &app, &wf));
        assert!(res.is_ok());
    }

    #[test]
    fn renders_overlay_idle_blank() {
        let app = make_overlay_app(false);
        let wf = WaveformHistory::new(40);
        let s = render_to_string(&app, &wf);
        assert!(s.contains("miccli"));
    }

    #[test]
    fn renders_overlay_recording() {
        let app = make_overlay_app(true);
        let mut wf = WaveformHistory::new(40);
        for _ in 0..10 {
            wf.push_level(70);
        }
        let s = render_to_string(&app, &wf);
        assert!(s.contains("REC") || s.contains("●"));
    }

    #[test]
    fn renders_overlay_paused() {
        let mut app = make_overlay_app(false);
        app.paused = true;
        let wf = WaveformHistory::new(40);
        let s = render_to_string(&app, &wf);
        assert!(s.contains("paused") || s.contains("⏸"));
    }
}
