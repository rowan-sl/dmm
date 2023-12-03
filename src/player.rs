pub mod output;
pub mod player;

use std::sync::Arc;

use color_eyre::eyre::{bail, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device, Host, SupportedStreamConfig,
};
use flume::Sender;
pub use player::{Command, DecodeAndPlay};
use symphonia::core::{io::MediaSourceStream, probe::Hint};
use tokio::{
    spawn,
    sync::oneshot,
    task::{spawn_local, JoinHandle},
};

pub struct Player {
    // audio output
    a_host: Host,
    a_device: Arc<Device>,
    a_config: Arc<SupportedStreamConfig>,
    // now playing task
    t_handle: Option<JoinHandle<Result<()>>>,
    t_notify: Option<Sender<player::Command>>,
}

impl Player {
    pub fn new_with_defaults() -> Result<Self> {
        debug!("Initializing audio backend");
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            error!("No audio output device exists!");
            bail!("failed to initialize audio backend");
        };
        let config = match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                error!("failed to get default audio output device config: {}", err);
                bail!("failed to initialize audio backend");
            }
        };
        Ok(Self {
            a_host: host,
            a_device: Arc::new(device),
            a_config: Arc::new(config),
            t_handle: None,
            t_notify: None,
        })
    }

    pub fn load_audio(
        &mut self,
        mss: MediaSourceStream,
        hint: Hint,
    ) -> Result<oneshot::Receiver<()>> {
        let (command_queue_sender, command_queue) = flume::bounded(1);
        let device = self.a_device.clone();
        let config = self.a_config.clone();
        let (done_sender, done_receiver) = oneshot::channel();
        self.t_notify = Some(command_queue_sender);
        // self.t_handle = Some(spawn(async move {
        //     let mut play = player::DecodeAndPlay::open(&device, &config, mss, hint, command_queue);
        //     let res = play.run().await;
        //     let _ = done_sender.send(());
        //     res
        // }));
        Ok(done_receiver)
    }
}
