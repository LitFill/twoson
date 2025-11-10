use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{collections::HashMap, error::Error, fs::File, io::{self, BufReader}, path::PathBuf};
use serde::Deserialize;
use tui_textarea::TextArea;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// File sumber JSON yang akan diterjemahkan
    source_file: PathBuf,

    /// File output untuk menyimpan hasil terjemahan.
    /// Jika tidak disediakan, akan menggunakan nama file sumber dengan prefix bahasa (misal: id_source.json)
    #[arg(short, long)]
    out: Option<PathBuf>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum JsonValue {
    String(String),
    Object(HashMap<String, JsonValue>),
}

type JsonData = HashMap<String, JsonValue>;

#[derive(Clone)]
struct TranslationItem {
    key: String,
    source_text: String,
    target_text: Option<String>,
}

impl TranslationItem {
    fn is_translated(&self) -> bool {
        self.target_text.is_some()
    }
}

struct App<'a> {
    items: Vec<TranslationItem>,
    selected_index: usize,
    textarea: TextArea<'a>,
}

impl<'a> App<'a> {
    fn new(items: Vec<TranslationItem>) -> App<'a> {
        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit Terjemahan"),
        );
        App {
            items,
            selected_index: 0,
            textarea,
        }
    }

    fn next(&mut self) {
        if self.selected_index < self.items.len() - 1 {
            self.selected_index += 1;
        }
    }

    fn previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }
}

fn load_translation_items(
    source_path: &PathBuf,
    output_path: Option<&PathBuf>,
) -> Result<Vec<TranslationItem>, Box<dyn Error>> {
    // Load source file
    let source_file = File::open(source_path)?;
    let reader = BufReader::new(source_file);
    let source_data: JsonData = serde_json::from_reader(reader)?;

    // Load target file if provided
    let mut target_data: JsonData = HashMap::new();
    if let Some(path) = output_path {
        if path.exists() {
            let target_file = File::open(path)?;
            let target_reader = BufReader::new(target_file);
            target_data = serde_json::from_reader(target_reader)?;
        }
    }

    // Helper function to flatten the nested JsonData
    fn flatten_json(data: &JsonData) -> HashMap<String, String> {
        let mut flat_map = HashMap::new();
        for (key, value) in data {
            flatten_recursive(key, value, &mut flat_map);
        }
        flat_map
    }

    fn flatten_recursive(prefix: &str, value: &JsonValue, flat_map: &mut HashMap<String, String>) {
        match value {
            JsonValue::String(s) => {
                flat_map.insert(prefix.to_string(), s.clone());
            }
            JsonValue::Object(obj) => {
                for (key, inner_value) in obj {
                    let new_prefix = format!("{}.{}", prefix, key);
                    flatten_recursive(&new_prefix, inner_value, flat_map);
                }
            }
        }
    }

    let flat_source_data = flatten_json(&source_data);
    let flat_target_data = flatten_json(&target_data);

    // Create TranslationItems
    let mut items: Vec<TranslationItem> = Vec::new();
    for (key, source_text) in flat_source_data {
        let target_text = flat_target_data.get(&key).cloned();
        items.push(TranslationItem {
            key,
            source_text,
            target_text,
        });
    }

    // Sort items by key for consistent display
    items.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(items)
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load translation items from files
    let items = match load_translation_items(&cli.source_file, cli.out.as_ref()) {
        Ok(items) => items,
        Err(e) => {
            // Restore terminal before printing error
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
            eprintln!("Error loading translation files: {}", e);
            return Err(e);
        }
    };

    // Buat app dan jalankan
    let app = App::new(items);
    let res = run_app(&mut terminal, app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error in TUI: {:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(f.area());

    // Panel Kiri: Daftar Kunci
    let items: Vec<ListItem> = app
        .items
        .iter()
        .map(|i| {
            let status = if i.is_translated() { "[✓]" } else { "[ ]" };
            let content = format!("{} {}", status, i.key);
            ListItem::new(content)
        })
        .collect();

    let items_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Daftar Kunci"))
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    // Ini adalah cara manual untuk mengelola state list, ratatui tidak punya state bawaan
    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(app.selected_index));

    f.render_stateful_widget(items_list, chunks[0], &mut list_state);

    // Panel Kanan
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    // Panel Kanan Atas: Teks Sumber
    let source_text = if let Some(item) = app.items.get(app.selected_index) {
        item.source_text.clone()
    } else {
        String::new()
    };
    let source_paragraph = Paragraph::new(source_text)
        .block(Block::default().borders(Borders::ALL).title("Teks Sumber"));
    f.render_widget(source_paragraph, right_chunks[0]);

    // Panel Kanan Bawah: Area Input
    f.render_widget(&app.textarea, right_chunks[1]);
}