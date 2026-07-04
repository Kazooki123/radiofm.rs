use crate::app::{App, InputMode};
use crate::source::NowPlaying;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame,
};

const ACCENT: Color = Color::Magenta;
const DIM: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // TITLE
            Constraint::Min(10),   // BODY
            Constraint::Length(3), // STATS/HELP
        ])
        .split(frame.area());

    draw_title(frame, root[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(root[1]);

    draw_station_list(frame, body[0], app);
    draw_now_playing(frame, body[1], app);

    draw_status_bar(frame, root[2], app);

    if app.input_mode != InputMode::None {
        draw_input_popup(frame, app);
    }
}

fn draw_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled(" radiofm.rs ", Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("\u{1F63C}", Style::default()),
        Span::styled("  Radio FM in your Terminal", Style::default().fg(DIM)),
    ]))
    .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(DIM)));
    frame.render_widget(title, area);
}

fn draw_station_list(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .stations
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let playing = is_currently_playing(app, i);
            let marker = if playing { "\u{25B6} " } else { "  " };
            let style = if playing {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(s.label(), style),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    if !app.stations.is_empty() {
        state.select(Some(app.selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title(" Stations "),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    frame.render_stateful_widget(list, area, &mut state);
}

fn is_currently_playing(app: &App, idx: usize) -> bool {
    let Some(station) = app.stations.get(idx) else {
        return false;
    };
    match (&app.player.now_playing, &station.kind) {
        (NowPlaying::Internet { url, .. }, crate::config::StationKind::Internet(u)) => url == u,
        (NowPlaying::Fm { khz, .. }, crate::config::StationKind::Fm(k)) => khz == k,
        _ => false,
    }
}

fn draw_now_playing(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    let now_text = match &app.player.now_playing {
        NowPlaying::Nothing => vec![
            Line::from(Span::styled("Nothing playing", Style::default().fg(DIM))),
            Line::from(Span::styled(
                "Press Enter on a station, or 'f' to type an FM frequency.",
                Style::default().fg(DIM),
            )),
        ],
        NowPlaying::Internet { name, url } => vec![
            Line::from(vec![
                Span::styled("\u{1F4E1} ", Style::default()),
                Span::styled(name.clone(), Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled(url.clone(), Style::default().fg(DIM))),
            Line::from(Span::styled("Mode: Internet stream", Style::default().fg(Color::Cyan))),
        ],
        NowPlaying::Fm { khz, hardware } => vec![
            Line::from(vec![
                Span::styled("\u{1F4FB} ", Style::default()),
                Span::styled(
                    format!("{:.1} MHz FM", *khz as f32 / 1000.0),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                if *hardware {
                    "Mode: FM hardware tuner"
                } else {
                    "Mode: FM (simulated - no RTL-SDR dongle detected)"
                },
                Style::default().fg(if *hardware { Color::Green } else { Color::Yellow }),
            )),
        ],
    };
    let now_playing = Paragraph::new(now_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DIM))
            .title(" Now Playing "),
    );
    frame.render_widget(now_playing, chunks[0]);

    let vol = (app.player.volume() * 100.0).round() as u16;
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title(" Volume (mouse scroll, +/-) "),
        )
        .gauge_style(Style::default().fg(ACCENT))
        .percent(vol.min(100));
    frame.render_widget(gauge, chunks[1]);

    draw_fm_dial(frame, chunks[2], app);
}

fn draw_fm_dial(frame: &mut Frame, area: Rect, app: &App) {
    use crate::source::fm::{FM_BAND_MAX_KHZ, FM_BAND_MIN_KHZ};

    let current_khz = match app.player.now_playing {
        NowPlaying::Fm { khz, .. } => khz,
        _ => app.config.last_fm_khz,
    };
    let span = (FM_BAND_MAX_KHZ - FM_BAND_MIN_KHZ) as f32;
    let frac = ((current_khz.saturating_sub(FM_BAND_MIN_KHZ)) as f32 / span).clamp(0.0, 1.0);

    let width = area.width.saturating_sub(4).max(10) as usize;
    let pos = (frac * (width as f32 - 1.0)).round() as usize;
    let mut dial: Vec<char> = vec!['\u{2500}'; width];
    if pos < dial.len() {
        dial[pos] = '\u{25CF}'; // ●
    }
    let dial_line: String = dial.into_iter().collect();

    let text = vec![
        Line::from(Span::styled(
            format!("{:.1}", FM_BAND_MIN_KHZ as f32 / 1000.0),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(dial_line, Style::default().fg(ACCENT))),
        Line::from(vec![
            Span::styled(format!("{:.1} MHz  ", current_khz as f32 / 1000.0), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("(\u{2190}/\u{2192} nudge 0.1MHz, f = type exact)", Style::default().fg(DIM)),
        ]),
    ];
    let dial_widget = Paragraph::new(text)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title(" FM Dial (87.5 - 108.0 MHz) "),
        );
    frame.render_widget(dial_widget, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let help = "\u{2191}/\u{2193} navigate  Enter play  s stop  a add station  d delete  f tune FM  \u{2190}/\u{2192} nudge FM  +/- volume  q quit";
    let text = vec![
        Line::from(Span::styled(app.status.clone(), Style::default().fg(Color::White))),
        Line::from(Span::styled(help, Style::default().fg(DIM))),
    ];
    let bar = Paragraph::new(text).block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    frame.render_widget(bar, area);
}

fn draw_input_popup(frame: &mut Frame, app: &App) {
    let area = centered_rect(60, 15, frame.area());
    let title = match &app.input_mode {
        InputMode::FmFrequency => " Tune FM (MHz) ",
        InputMode::NewStationName => " New Station: Name ",
        InputMode::NewStationUrl { .. } => " New Station: Stream URL ",
        InputMode::None => "",
    };
    let text = format!("{}\u{2588}", app.input_buffer);
    let popup = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT))
            .title(title),
    );
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(popup, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
