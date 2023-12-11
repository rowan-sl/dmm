use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use color_eyre::eyre::Result;
use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use flume::{Receiver, Sender};
use ratatui::backend::CrosstermBackend as Backend;
use serde::{Deserialize, Serialize};

pub type IO = std::io::Stdout;
pub fn io() -> IO {
    std::io::stdout()
}
pub type Frame<'a> = ratatui::Frame<'a>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Init,
    Quit,
    Error,
    Closed,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

pub struct Tui {
    pub terminal: ratatui::Terminal<Backend<IO>>,
    pub task: Option<JoinHandle<()>>,
    pub close_flag: Arc<AtomicBool>,
    pub event_rx: Receiver<Event>,
    pub event_tx: Sender<Event>,
    pub frame_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let frame_rate = 30.0;
        let terminal = ratatui::Terminal::new(Backend::new(io()))?;
        let (event_tx, event_rx) = flume::unbounded();
        Ok(Self {
            terminal,
            task: None,
            close_flag: Arc::new(AtomicBool::new(false)),
            event_rx,
            event_tx,
            frame_rate,
            mouse: false,
            paste: false,
        })
    }

    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    #[allow(unused)]
    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = mouse;
        self
    }

    #[allow(unused)]
    pub fn paste(mut self, paste: bool) -> Self {
        self.paste = paste;
        self
    }

    pub fn start(&mut self) {
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);
        self.cancel();
        self.close_flag.store(false, Ordering::Relaxed);
        let close_flag = self.close_flag.clone();
        let event_tx = self.event_tx.clone();
        self.task = Some(
            thread::Builder::new()
                .name(String::from("tui-event-listen"))
                .spawn(move || {
                    event_tx.send(Event::Init).unwrap();
                    let mut last_time = Instant::now();
                    let mut sleep_amnt = render_delay;
                    loop {
                        if event::poll(sleep_amnt).unwrap_or_else(|e| {
                            error!("Error reading event: {e:?}");
                            event_tx.send(Event::Error).unwrap();
                            false
                        }) {
                            // event
                            match event::read() {
                                Ok(evt) => match evt {
                                    CrosstermEvent::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            event_tx.send(Event::Key(key)).unwrap();
                                        }
                                    }
                                    CrosstermEvent::Mouse(mouse) => {
                                        event_tx.send(Event::Mouse(mouse)).unwrap();
                                    }
                                    CrosstermEvent::Resize(x, y) => {
                                        event_tx.send(Event::Resize(x, y)).unwrap();
                                    }
                                    CrosstermEvent::FocusLost => {
                                        event_tx.send(Event::FocusLost).unwrap();
                                    }
                                    CrosstermEvent::FocusGained => {
                                        event_tx.send(Event::FocusGained).unwrap();
                                    }
                                    CrosstermEvent::Paste(s) => {
                                        event_tx.send(Event::Paste(s)).unwrap();
                                    }
                                },
                                Err(e) => {
                                    error!("Error reading event: {e:?}");
                                    event_tx.send(Event::Error).unwrap();
                                }
                            }
                        }
                        // -- note --
                        // this may appear to cause issues (high framerate when pressing buttons quickly)
                        // in reality, this allows for a very low framerate (10fps and still have good input feel)
                        // by rendering a frame when you give an input.
                        // do NOT fix this
                        event_tx.send(Event::Render).unwrap();
                        if close_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        // dynamically adjust sleep time to maintain a steady framerate
                        let now = Instant::now();
                        sleep_amnt = render_delay
                            .saturating_sub(last_time.elapsed().saturating_sub(sleep_amnt));
                        last_time = now;
                    }
                })
                .unwrap(),
        );
    }

    pub fn stop(&mut self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        while !task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > (2.0 * self.frame_rate * 1000.0) as _ {
                log::error!("Failed to kill task within 2 frames");
                break;
            }
        }
        Ok(())
    }

    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(io(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(io(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(io(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.paste {
                crossterm::execute!(io(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(io(), DisableMouseCapture)?;
            }
            crossterm::execute!(io(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.close_flag.store(true, Ordering::Relaxed);
    }

    pub fn next(&mut self) -> Option<Event> {
        self.event_rx.recv().ok()
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<IO>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.exit().unwrap();
    }
}
