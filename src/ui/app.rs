use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::prelude::Rect;

use super::{
    action::Action,
    components::{
        fps::FpsCounter,
        home::{Home, PlaylistID},
        Component,
    },
    mode::Mode,
    tui,
};
use crate::resolver::Resolver;

pub struct App {
    pub frame_rate: f64,
    pub components: Vec<Box<dyn Component>>,
    pub should_quit: bool,
    pub mode: Mode,
    pub last_tick_key_events: Vec<KeyEvent>,
    pub resolver: Arc<Resolver>,
}

impl App {
    pub fn new(res: Resolver, frame_rate: f64, pl: PlaylistID) -> Result<Self> {
        let resolver = Arc::new(res);
        let home = Home::new(pl, resolver.clone())?;
        let fps = FpsCounter::default();
        let mode = Mode::Home;
        Ok(Self {
            frame_rate,
            components: vec![Box::new(home), Box::new(fps)],
            should_quit: false,
            mode,
            last_tick_key_events: Vec::new(),
            resolver,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let (action_tx, action_rx) = flume::unbounded();

        let mut tui = tui::Tui::new()?.frame_rate(self.frame_rate);
        tui.enter()?;

        for component in self.components.iter_mut() {
            component.register_action_handler(action_tx.clone())?;
        }

        for component in self.components.iter_mut() {
            component.register_config_handler(self.resolver.out().config.clone())?;
        }

        for component in self.components.iter_mut() {
            component.init(tui.size()?)?;
        }

        loop {
            if let Some(e) = tui.next() {
                match e {
                    tui::Event::Quit => action_tx.send(Action::Quit)?,
                    tui::Event::Render => action_tx.send(Action::Render)?,
                    tui::Event::Resize(x, y) => action_tx.send(Action::Resize(x, y))?,
                    tui::Event::Key(key) => {
                        if let Some(keymap) = self.resolver.out().config.keybinds.get(&self.mode) {
                            if let Some(action) = keymap.get(&vec![key]) {
                                log::info!("Got action: {action:?}");
                                action_tx.send(action.clone())?;
                            } else {
                                // If the key was not handled as a single key action,
                                // then consider it for multi-key combinations.
                                self.last_tick_key_events.push(key);

                                // Check for multi-key combinations
                                if let Some(action) = keymap.get(&self.last_tick_key_events) {
                                    log::info!("Got action: {action:?}");
                                    action_tx.send(action.clone())?;
                                }
                            }
                        };
                    }
                    _ => {}
                }
                for component in self.components.iter_mut() {
                    if let Some(action) = component.handle_events(Some(e.clone()))? {
                        action_tx.send(action)?;
                    }
                }
            }

            while let Ok(action) = action_rx.try_recv() {
                if action != Action::Render {
                    log::debug!("{action:?}");
                }
                match action {
                    Action::Quit => self.should_quit = true,
                    Action::Resize(w, h) => {
                        tui.resize(Rect::new(0, 0, w, h))?;
                        let mut errors = vec![];
                        tui.draw(|f| {
                            for component in self.components.iter_mut() {
                                if let Err(e) = component.draw(f, f.size()) {
                                    errors.push(e);
                                }
                            }
                        })?;
                        if !errors.is_empty() {
                            Err(errors.remove(0))?
                        }
                    }
                    Action::Render => {
                        let mut errors = vec![];
                        tui.draw(|f| {
                            for component in self.components.iter_mut() {
                                if let Err(e) = component.draw(f, f.size()) {
                                    errors.push(e);
                                }
                            }
                        })?;
                        if !errors.is_empty() {
                            Err(errors.remove(0))?
                        }
                    }
                    _ => {}
                }
                for component in self.components.iter_mut() {
                    if let Some(action) = component.update(action.clone())? {
                        action_tx.send(action)?
                    };
                }
            }
            if self.should_quit {
                tui.stop()?;
                break;
            }
        }
        tui.exit()?;
        Ok(())
    }
}
