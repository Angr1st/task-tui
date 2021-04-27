use std::{fs, io, sync::mpsc, thread, time::{Duration, Instant}};
use crossterm::{event,event::KeyCode, terminal::{disable_raw_mode, enable_raw_mode}};
use rand::prelude::*;
use rand::distributions::Alphanumeric;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use thiserror::Error;
use tui::{Terminal, backend::CrosstermBackend, layout::{Alignment, Constraint, Direction, Layout}, style::{Color, Modifier, Style}, text::{Span, Spans}, widgets::{Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs}};

const DB_PATH: &str = "./data/db.json";

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: usize,
    name: String,
    state: String,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("error reading the DB file: {0}")]
    ReadDBError(#[from] io::Error),
    #[error("error parsing the DB file: {0}")]
    ParseDBError(#[from] serde_json::Error),
}

enum Event<I> {
    Input(I),
    Tick
}

#[derive(Copy, Clone, Debug)]
enum MenuItem {
    Home,
    Tasks
}

impl From<MenuItem> for usize {
    fn from(input: MenuItem) -> usize {
        match input {
            MenuItem::Home => 0,
            MenuItem::Tasks => 1
        }
    }
}

fn read_db() -> Result<Vec<Task>, Error> {
    let db_content = fs::read_to_string(DB_PATH)?;
    let parsed: Vec<Task> = serde_json::from_str(&db_content)?;
    Ok(parsed)
}

fn add_random_task_to_db() -> Result<Vec<Task>, Error> {
    let mut rng = rand::thread_rng();
    let db_content = fs::read_to_string(DB_PATH)?;
    let mut parsed: Vec<Task> = serde_json::from_str(&db_content)?;
    let task_category = match rng.gen_range(0..3) {
        0 => "pending",
        1 => "started",
        2 => "in progress",
        _ => "done"
    };

    let random_task = Task {
        id: rng.gen_range(1..99999),
        name: rng.sample_iter(Alphanumeric).take(10).map(char::from).collect(),
        state: task_category.to_owned(),
        created_at: Utc::now(),
        finished_at: None 
    };

    parsed.push(random_task);
    fs::write(DB_PATH, &serde_json::to_vec(&parsed)?)?;
    Ok(parsed)
}

fn remove_task_at_index(task_list_state: &mut ListState) -> Result<(), Error> {
    if let Some(selected) = task_list_state.selected() {
        let db_content = fs::read_to_string(DB_PATH)?;
        let mut parsed: Vec<Task> = serde_json::from_str(&db_content)?;
        parsed.remove(selected);
        fs::write(DB_PATH, &serde_json::to_vec(&parsed)?)?;
        task_list_state.select(Some(selected - 1));
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
        Spans::from(vec![Span::raw("Press 't' to access tasks, 'a' to add random new tasks, 'c' to complete the currently selected task and 'd' to delete the the currently selected task.")])

    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Home")
            .border_type(BorderType::Plain)
    );

    home
}

fn render_tasks<'a>(task_list_state: &ListState) -> (List<'a>, Table<'a>) {
    let tasks = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White))
        .title("Tasks")
        .border_type(BorderType::Plain);

    let task_list = read_db().expect("can fetch task list");
    let items: Vec<_> = task_list
        .iter()
        .map(|task| {
            ListItem::new(Spans::from(vec![Span::styled(
                task.name.clone(),
                Style::default()
            )]))
        })
        .collect();

    let selected_task = task_list
        .get(
            task_list_state
                .selected()
                .expect("there is always a selected task")
        )
        .expect("exists")
        .clone();

    let list = List::new(items).block(tasks).highlight_style(
        Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    );

    let task_detail = match selected_task.finished_at {
        Some(finished) => Table::new(vec![Row::new(vec![
            Cell::from(Span::raw(selected_task.id.to_string())),
            Cell::from(Span::raw(selected_task.name)),
            Cell::from(Span::raw(selected_task.state)),
            Cell::from(Span::raw(selected_task.created_at.to_string())),
            Cell::from(Span::raw(finished.to_string()))
        ])])
        .header(Row::new(vec![
            Cell::from(Span::styled(
                "ID",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "Name",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "State",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "Created At",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "Finished At",
                Style::default().add_modifier(Modifier::BOLD)
            ))
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White))
                .title("Detail")
                .border_type(BorderType::Plain)
        )
        .widths(&[
            Constraint::Percentage(5),
            Constraint::Percentage(30),
            Constraint::Percentage(10),
            Constraint::Percentage(20),
            Constraint::Percentage(20)
        ]),
        None => Table::new(vec![Row::new(vec![
            Cell::from(Span::raw(selected_task.id.to_string())),
            Cell::from(Span::raw(selected_task.name)),
            Cell::from(Span::raw(selected_task.state)),
            Cell::from(Span::raw(selected_task.created_at.to_string()))
        ])])
        .header(Row::new(vec![
            Cell::from(Span::styled(
                "ID",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "Name",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "State",
                Style::default().add_modifier(Modifier::BOLD)
            )),
            Cell::from(Span::styled(
                "Created At",
                Style::default().add_modifier(Modifier::BOLD)
            ))
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White))
                .title("Detail")
                .border_type(BorderType::Plain)
        )
        .widths(&[
            Constraint::Percentage(5),
            Constraint::Percentage(30),
            Constraint::Percentage(10),
            Constraint::Percentage(20)
        ])
    };

    (list,task_detail)     
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode().expect("can run in raw mode");

    let (tx,rx) = mpsc::channel();
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

    let menu_titles = vec!["Home", "Tasks", "Add", "Complete", "Delete", "Exit"];
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
                        Constraint::Max(3)
                    ]
                    .as_ref()
                )
                .split(size);

            let menu = menu_titles
                .iter()
                .map(|t| {
                    let (first,rest) =t.split_at(1);
                    Spans::from(vec![
                        Span::styled(
                            first,
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::UNDERLINED)
                        ),
                        Span::styled(rest, Style::default().fg(Color::White))
                    ])
                })
                .collect();

            let tabs = Tabs::new(menu)
                .select(active_menu_item.into())
                .block(Block::default().title("Menu").borders(Borders::ALL))
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().fg(Color::Yellow))
                .divider(Span::raw("|"));

            rect.render_widget(tabs, chunks[0]);

            let copyright = Paragraph::new("task-TUI 2021 - all rights reserved")
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::White))
                        .title("Copyright")
                        .border_type(BorderType::Plain)
                );

            rect.render_widget(copyright, chunks[2]);

            match active_menu_item {
                MenuItem::Home => rect.render_widget(render_home(), chunks[1]),
                MenuItem::Tasks => {
                    let task_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [Constraint::Percentage(20), Constraint::Percentage(80)].as_ref()
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
                KeyCode::Char('q') => {
                    disable_raw_mode()?;
                    terminal.show_cursor()?;
                    break;
                }
                KeyCode::Char('h') => active_menu_item = MenuItem::Home,
                KeyCode::Char('t') => active_menu_item = MenuItem::Tasks,
                KeyCode::Char('a') => {
                    add_random_task_to_db().expect("can add new random task");
                } 
                KeyCode::Char('d') => {
                    remove_task_at_index(&mut task_list_state).expect("can remove task");
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
