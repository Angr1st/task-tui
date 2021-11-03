use chrono::{DateTime, Utc};
use crossterm::{
    event,
    event::KeyCode,
    terminal::{disable_raw_mode, enable_raw_mode},
};

use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, fs::{File, OpenOptions}, io::{self, Read, Seek, SeekFrom}, path::PathBuf, sync::mpsc, thread, time::{Duration, Instant}, usize};
use thiserror::Error;
use tui::{Terminal, backend::CrosstermBackend, layout::{Alignment, Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Span, Spans}, widgets::{Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs}};
use unicode_width::UnicodeWidthStr;

const DB_PATH: &str = "./data/db.json";

fn find_default_db_file() -> Option<PathBuf> {
    home::home_dir().map(|mut path| {
        path.push(DB_PATH);
        path
    })
}

fn ensure_db_file_exists(path: PathBuf) -> Result<File, Error> {
    let file = {
        if path.exists() && path.is_file() {
            OpenOptions::new().read(true).write(true).open(path)?
        } else {
            match path.parent() {
                Some(parent) => std::fs::create_dir_all(parent)?,
                None => (),
            };

            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)?
        }
    };

    Ok(file)
}

#[derive(PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

/// App holds the state of the application
struct App {
    /// Current value of the input box
    input: String,
    /// Current input mode
    input_mode: InputMode
}

impl Default for App {
    fn default() -> App {
        App {
            input: String::new(),
            input_mode: InputMode::Normal
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
enum TaskState {
    Pending,
    Started,
    InProgress,
    Done,
}

impl TaskState {
    fn new() -> TaskState {
        TaskState::Pending
    }

    fn to_string(&self) -> String {
        {
            match self {
                TaskState::Pending => "pending",
                TaskState::Started => "started",
                TaskState::InProgress => "in progress",
                TaskState::Done => "done",
            }
        }
        .to_string()
    }

    fn progress(&mut self) -> Self {
        match self {
            TaskState::Pending => TaskState::Started,
            TaskState::Started => TaskState::InProgress,
            TaskState::InProgress => TaskState::Done,
            TaskState::Done => TaskState::Done,
        }
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<&str> for TaskState {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(match value {
            "pending" => TaskState::Pending,
            "started" => TaskState::Started,
            "in progress" => TaskState::InProgress,
            "done" => TaskState::Done,
            _ => {
                return Err(Self::Error::StringError(String::from(
                    "input was not a valid TaskState",
                )))
            }
        })
    }
}

impl TryFrom<usize> for TaskState {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => TaskState::Pending,
            1 => TaskState::Started,
            2 => TaskState::InProgress,
            3 => TaskState::Done,
            _ => {
                return Err(Self::Error::StringError(String::from(
                    "input was not a valid TaskState",
                )))
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: usize,
    name: String,
    state: TaskState,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl Task {
    fn create_task(number:usize,task_name:String) -> Task {
        let task_state = TaskState::new();

        Task {
            id: number,
            name: task_name,
            state: task_state,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        }
    }

    fn progress(&mut self) {
        self.state = self.state.progress();
        match self.state {
            TaskState::Started => self.started_at = Some(Utc::now()),
            TaskState::Done => self.finished_at = Some(Utc::now()),
            _ => {}
        }
    }

    fn create_table_row<'a>(&self) -> Row<'a> {
        let mut cell_vec = vec![
            Cell::from(Span::raw(self.id.to_string())),
            Cell::from(Span::raw(self.name.clone())),
            Cell::from(Span::raw(self.state.to_string())),
            Cell::from(Span::raw(self.created_at.to_string())),
        ];

        if let Some(started_at) = self.started_at {
            cell_vec.push(Cell::from(Span::raw(started_at.to_string())));
        }

        if let Some(finished) = self.finished_at {
            cell_vec.push(Cell::from(Span::raw(finished.to_string())));
        }

        Row::new(cell_vec)
    }

    fn create_header_row<'a>(&self) -> Row<'a> {
        let mut cell_vec = vec![
            Cell::from(Span::styled(
                "ID",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Name",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "State",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Created At",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ];

        if let Some(_) = self.started_at {
            cell_vec.push(Cell::from(Span::styled(
                "Started At",
                Style::default().add_modifier(Modifier::BOLD),
            )));
        }

        if let Some(_) = self.finished_at {
            cell_vec.push(Cell::from(Span::styled(
                "Finished At",
                Style::default().add_modifier(Modifier::BOLD),
            )));
        }

        Row::new(cell_vec)
    }

    fn create_block_constraints<'a>(self) -> &'a [Constraint] {
        match self.state {
            TaskState::Pending => &[
                Constraint::Percentage(5),
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Percentage(19),
            ],
            TaskState::Started => &[
                Constraint::Percentage(5),
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Percentage(19),
                Constraint::Percentage(19),
            ],
            TaskState::InProgress => &[
                Constraint::Percentage(5),
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Percentage(19),
                Constraint::Percentage(19),
            ],
            TaskState::Done => &[
                Constraint::Percentage(5),
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Percentage(19),
                Constraint::Percentage(19),
                Constraint::Percentage(19),
            ],
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("error reading the DB file: {0}")]
    ReadDBError(#[from] io::Error),
    #[error("error parsing the DB file: {0}")]
    ParseDBError(#[from] serde_json::Error),
    #[error("error: {0}")]
    StringError(String),
}

enum Event<I> {
    Input(I),
    Tick,
}

#[derive(Copy, Clone, Debug)]
enum MenuItem {
    Home,
    Tasks,
}

impl From<MenuItem> for usize {
    fn from(input: MenuItem) -> usize {
        match input {
            MenuItem::Home => 0,
            MenuItem::Tasks => 1,
        }
    }
}

impl From<MenuItem> for &str {
    fn from(input: MenuItem) -> &'static str {
        match input {
            MenuItem::Home => "Home",
            MenuItem::Tasks => "Tasks",
        }
    }
}

impl From<MenuItem> for UiSections {
    fn from(input: MenuItem) -> UiSections {
        UiSections::MenuItem(input)
    }
}

#[derive(Copy, Clone, Debug)]
enum UiSections {
    Detail,
    Copyright,
    Menu,
    MenuItem(MenuItem),
}

impl From<UiSections> for &str {
    fn from(input: UiSections) -> &'static str {
        match input {
            UiSections::Detail => "Detail",
            UiSections::Copyright => "Copyright",
            UiSections::Menu => "Menu",
            UiSections::MenuItem(menu_item) => menu_item.into(),
        }
    }
}

fn collect_tasks(mut file: &File) -> Result<Vec<Task>, Error> {
    file.seek(SeekFrom::Start(0))?; // Rewind the file before.
    let mut s = String::new();
    file.read_to_string(&mut s)?;

    if s == "null" {
        return Ok(Vec::new())
    }

    let tasks = match serde_json::from_str(&s) {
        Ok(tasks) => tasks,
        Err(e) if e.is_eof() => Vec::new(),
        Err(e) => Err(e)?,
    };
    file.seek(SeekFrom::Start(0))?; // Rewind the file after.
    Ok(tasks)
}

fn read_db() -> Result<Vec<Task>, Error> {
    let db_file = get_db_file()?;

    collect_tasks(&db_file)
}

fn get_db_file() -> Result<File, Error> {
    let db_path = find_default_db_file().expect("Task db file should be found!");
    let db_file = ensure_db_file_exists(db_path)?;
    Ok(db_file)
}

fn write_db(mut tasks: Vec<Task>) -> Result<Vec<Task>, Error> {
    let db_file = get_db_file()?;

    db_file.set_len(0)?;
    tasks.sort_by(|a,b| a.id.cmp(&b.id));
    serde_json::to_writer(db_file, &tasks)?;
    Ok(tasks)
}

fn add_task_to_db(name:String) -> Result<Vec<Task>, Error> {
    let mut parsed: Vec<Task> = read_db()?;
    let new_task = if parsed.len() != 0 
    {
        let highest_id = parsed.last().map_or(1, |a| a.id) + 1;
        Task::create_task(highest_id,name)
    }
    else {
        Task::create_task(1,name)
    };

    parsed.push(new_task);

    let parsed = write_db(parsed)?;
    Ok(parsed)
}

fn progress_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let mut parsed: Vec<Task> = read_db()?;
        if parsed.len() > 0 {
            let element = &mut parsed[selected];
            element.progress();
            write_db(parsed)?;
        }
    }

    Ok(())
}

fn remove_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let mut parsed: Vec<Task> = read_db()?;
        if parsed.len() > 0 {
            parsed.remove(selected);
            write_db(parsed)?;
            if selected != 0 {
                task_list_state.select(Some(selected - 1));
            }
        }
    }

    Ok(())
}

fn render_home<'a>() -> Paragraph<'a> {
    let home = Paragraph::new(vec![
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Welcome")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("to")]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::styled(
            "task-TUI",
            Style::default().fg(Color::White),
        )]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Press 't' to access tasks,")]),
        Spans::from(vec![Span::raw("'a' to add random new tasks,")]),
        Spans::from(vec![Span::raw(
            "'p' to progress the currently selected task",
        )]),
        Spans::from(vec![Span::raw(
            "'d' to delete the the currently selected task.",
        )]),
    ])
    .alignment(Alignment::Center)
    .block(create_default_table_block(MenuItem::Home.into()));

    home
}

fn render_tasks<'a>(task_list_state: &ListState) -> (List<'a>, Table<'a>) {
    let tasks = create_default_table_block(MenuItem::Tasks.into());

    let task_list = read_db().expect("can fetch task list");
    let items: Vec<_> = task_list
        .iter()
        .map(|task| {
            ListItem::new(Spans::from(vec![Span::styled(
                task.name.clone(),
                Style::default(),
            )]))
        })
        .collect();

    let selected_task = task_list
        .get(task_list_state.selected().unwrap_or(0))
        .map(|f| f.clone());

    let list = List::new(items).block(tasks).highlight_style(
        Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );

    let task_detail = match selected_task {
        Some(inner_task) => Table::new(vec![inner_task.create_table_row()])
            .header(inner_task.create_header_row())
            .block(create_default_table_block(UiSections::Detail.into()))
            .widths(inner_task.create_block_constraints()),
        None => create_empty_table(),
    };

    (list, task_detail)
}

fn create_default_table_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White))
        .title(title)
        .border_type(BorderType::Plain)
}

fn create_empty_table<'a>() -> Table<'a> {
    Table::new(vec![Row::new(vec![Cell::from(Span::raw(""))])])
        .header(Row::new(vec![Cell::from(Span::styled(
            "",
            Style::default().add_modifier(Modifier::BOLD),
        ))]))
        .block(create_default_table_block(UiSections::Detail.into()))
        .widths(&[Constraint::Percentage(70)])
}

/// helper function to create a centered rect using up
/// certain percentage of the available rect `r`
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode().expect("can run in raw mode");

    let (tx, rx) = mpsc::channel();
    let tick_rate = Duration::from_millis(200);
    thread::spawn(move || {
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout).expect("poll works") {
                if let event::Event::Key(key) = event::read().expect("can read events") {
                    tx.send(Event::Input(key)).expect("can send events");
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if let Ok(_) = tx.send(Event::Tick) {
                    last_tick = Instant::now();
                }
            }
        }
    });

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Create default app state
    let mut app = App::default();

    let menu_titles = vec!["Home", "Tasks", "Add", "Progress", "Delete", "Exit"];
    let mut active_menu_item = MenuItem::Home;

    let mut task_list_state = ListState::default();
    task_list_state.select(Some(0));

    loop {
        terminal.draw(|rect| {
            let size = rect.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Min(2),
                        Constraint::Max(3),
                    ]
                    .as_ref(),
                )
                .split(size);

            let menu = menu_titles
                .iter()
                .map(|t| {
                    let (first, rest) = t.split_at(1);
                    Spans::from(vec![
                        Span::styled(
                            first,
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::UNDERLINED),
                        ),
                        Span::styled(rest, Style::default().fg(Color::White)),
                    ])
                })
                .collect();

            let tabs = Tabs::new(menu)
                .select(active_menu_item.into())
                .block(create_default_table_block(UiSections::Menu.into()))
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().fg(Color::Yellow))
                .divider(Span::raw("|"));

            rect.render_widget(tabs, chunks[0]);

            let copyright = Paragraph::new("task-TUI 2021 - all rights reserved")
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Center)
                .block(create_default_table_block(UiSections::Copyright.into()));

            rect.render_widget(copyright, chunks[2]);

            match active_menu_item {
                MenuItem::Home => rect.render_widget(render_home(), chunks[1]),
                MenuItem::Tasks => {
                    let task_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [Constraint::Percentage(20), Constraint::Percentage(80)].as_ref(),
                        )
                        .split(chunks[1]);
                    let (left, right) = render_tasks(&task_list_state);
                    rect.render_stateful_widget(left, task_chunks[0], &mut task_list_state);
                    rect.render_widget(right, task_chunks[1]);
                }
            }

            if app.input_mode == InputMode::Editing {
                //let block = Block::default().title("Popup").borders(Borders::ALL);
                let input = Paragraph::new(app.input.as_ref())
                .style(match app.input_mode {
                    InputMode::Normal => Style::default(),
                    InputMode::Editing => Style::default().fg(Color::Yellow),
                })
                .block(Block::default().borders(Borders::ALL).title("Input"));
                
                let area = centered_rect(60, 10, size);
                rect.render_widget(Clear, area); //this clears out the background
                rect.render_widget(input, area);
            }

            match app.input_mode {
                InputMode::Normal =>
                    // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                    {}

                InputMode::Editing => {
                    let area = centered_rect(60, 10, size);

                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    rect.set_cursor(
                        // Put cursor past the end of the input text
                        area.x + app.input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        area.y + 1,
                    )
                }
            }

        })?;

        match rx.recv()? {
            Event::Input(event) => 
                match app.input_mode {
                    InputMode::Normal => {
                        match event.code {
                            KeyCode::Char('e') => {
                                disable_raw_mode()?;
                                terminal.show_cursor()?;
                                terminal.clear()?;
                                break;
                            }
                            KeyCode::Char('h') => active_menu_item = MenuItem::Home,
                            KeyCode::Char('t') => active_menu_item = MenuItem::Tasks,
                            KeyCode::Char('a') => {
                                app.input_mode = InputMode::Editing;
                                //add_task_to_db()?;
                            }
                            KeyCode::Char('p') => {
                                progress_task_at_index(&mut task_list_state)?;
                            }
                            KeyCode::Char('d') => {
                                remove_task_at_index(&mut task_list_state)?;
                            }
                            KeyCode::Down => {
                                if let Some(selected) = task_list_state.selected() {
                                    let amount_task = read_db().expect("can fetch task list").len();
                                    if selected >= amount_task - 1 {
                                        task_list_state.select(Some(0));
                                    } else {
                                        task_list_state.select(Some(selected + 1));
                                    }
                                }
                            }
                            KeyCode::Up => {
                                if let Some(selected) = task_list_state.selected() {
                                    let amount_task = read_db().expect("can fetch task list").len();
                                    if selected > 0 {
                                        task_list_state.select(Some(selected - 1));
                                    } else {
                                        task_list_state.select(Some(amount_task - 1));
                                    }
                                }
                            }
                            _ => {}
                    }
                }
                InputMode::Editing => {
                    match event.code {
                        KeyCode::Enter => {
                            add_task_to_db(app.input.drain(..).collect())?;
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Char(c) => {
                            app.input.push(c);
                        }
                        KeyCode::Backspace => {
                            app.input.pop();
                        }
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                        }
                        _ => {}
                    }
                }
            },
            Event::Tick => {}
        }
    }

    Ok(())
}
