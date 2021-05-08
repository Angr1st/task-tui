use chrono::{Date, DateTime, Utc};
use crossterm::{
    event,
    event::KeyCode,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use rand::distributions::Alphanumeric;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    fs::{File, OpenOptions},
    io::{self, Seek, SeekFrom},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
    usize,
};
use thiserror::Error;
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{
        Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
    },
    Terminal,
};

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

#[derive(Serialize, Deserialize, Clone)]
enum TaskState {
    Pending,
    Started,
    InProgress,
    Done,
}

impl TaskState {
    fn new(mut rng: ThreadRng) -> TaskState {
        TaskState::try_from(rng.gen_range(0..3)).expect("The range from 0 to 3 should be correct!")
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
    fn create_random_task() -> Task {
        let mut rng = rand::thread_rng();

        let task_category = TaskState::new(rng.clone());

        Task {
            id: rng.gen_range(1..99999),
            name: rng
                .sample_iter(Alphanumeric)
                .take(10)
                .map(char::from)
                .collect(),
            state: task_category,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        }
    }

    fn progress(&mut self) {
        self.state = self.state.progress();
    }

    fn create_table_row<'a>(self) -> Row<'a> {
        let mut cell_vec = vec![
            Cell::from(Span::raw(self.id.to_string())),
            Cell::from(Span::raw(self.name)),
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
    let tasks = match serde_json::from_reader(file) {
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

fn write_db(tasks: Vec<Task>) -> Result<Vec<Task>, Error> {
    let db_file = get_db_file()?;

    db_file.set_len(0)?;

    serde_json::to_writer(db_file, &tasks)?;
    Ok(tasks)
}

fn add_random_task_to_db() -> Result<Vec<Task>, Error> {
    let mut parsed: Vec<Task> = read_db()?;
    parsed.push(Task::create_random_task());

    let parsed = write_db(parsed)?;
    Ok(parsed)
}

fn progress_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let mut parsed: Vec<Task> = read_db()?;
        let element = &mut parsed[selected];
        element.progress();
        write_db(parsed)?;
    }

    Ok(())
}

fn remove_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let mut parsed: Vec<Task> = read_db()?;
        parsed.remove(selected);
        write_db(parsed)?;
        if selected != 0 {
            task_list_state.select(Some(selected - 1));
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
            Style::default().fg(Color::White)
        )]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Press 't' to access tasks,")]),
        Spans::from(vec![Span::raw("'a' to add random new tasks,")]),
        Spans::from(vec![Span::raw("'p' to progress the currently selected task")]),
        Spans::from(vec![Span::raw("'d' to delete the the currently selected task.")])

    ])
    .alignment(Alignment::Center)
    .block(
        create_default_table_block(MenuItem::Home.into())
    );

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
        Some(inner_task) => match inner_task.finished_at {
            Some(finished) => Table::new(vec![inner_task.create_table_row()])
                .header(Row::new(vec![
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
                    Cell::from(Span::styled(
                        "Finished At",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                ]))
                .block(create_default_table_block(UiSections::Detail.into()))
                .widths(&[
                    Constraint::Percentage(5),
                    Constraint::Percentage(30),
                    Constraint::Percentage(10),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                ]),
            None => Table::new(vec![Row::new(vec![
                Cell::from(Span::raw(inner_task.id.to_string())),
                Cell::from(Span::raw(inner_task.name)),
                Cell::from(Span::raw(inner_task.state.to_string())),
                Cell::from(Span::raw(inner_task.created_at.to_string())),
            ])])
            .header(Row::new(vec![
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
            ]))
            .block(create_default_table_block(UiSections::Detail.into()))
            .widths(&[
                Constraint::Percentage(5),
                Constraint::Percentage(30),
                Constraint::Percentage(10),
                Constraint::Percentage(20),
            ]),
        },
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
        })?;

        match rx.recv()? {
            Event::Input(event) => match event.code {
                KeyCode::Char('e') => {
                    disable_raw_mode()?;
                    terminal.show_cursor()?;
                    terminal.clear()?;
                    break;
                }
                KeyCode::Char('h') => active_menu_item = MenuItem::Home,
                KeyCode::Char('t') => active_menu_item = MenuItem::Tasks,
                KeyCode::Char('a') => {
                    add_random_task_to_db()?;
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
            },
            Event::Tick => {}
        }
    }

    Ok(())
}
