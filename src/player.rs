mod clock;
mod video;

extern crate ffmpeg_the_third as ffmpeg;

use futures::{future::OptionFuture, FutureExt};
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub enum ControlCommand {
    Play,
    Pause,
}

#[derive(Default)]
pub struct Player {
    control_sender: Option<smol::channel::Sender<ControlCommand>>,
    demuxer_thread: Option<std::thread::JoinHandle<()>>,
    playing: bool,
    playing_changed_callback: Option<Box<dyn Fn(bool)>>,
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(control_sender) = self.control_sender.take() {
            control_sender.close();
        }

        if let Some(decoder_thread) = self.demuxer_thread.take() {
            decoder_thread.join().unwrap();
        }
    }
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(
        &mut self,
        path: PathBuf,
        video_frame_callback: impl FnMut(&ffmpeg::util::frame::Video) + Send + 'static,
        playing_changed_callback: impl Fn(bool) + 'static,
    ) -> Result<(), anyhow::Error> {
        let (control_sender, control_receiver) = smol::channel::unbounded::<ControlCommand>();
        let demuxer_thread = std::thread::Builder::new()
            .name("demuxer thread".into())
            .spawn(move || {
                smol::block_on(async move {
                    let mut input_context = ffmpeg::format::input(&path).unwrap();

                    let video_stream = input_context
                        .streams()
                        .best(ffmpeg::media::Type::Video)
                        .unwrap();

                    let video_stream_index = video_stream.index();
                    let video_playback_thread = video::VideoPlaybackThread::start(
                        &video_stream,
                        Box::new(video_frame_callback),
                    )
                    .unwrap();

                    // i don't do the audio for now

                    let mut playing = true;

                    let packet_forwarder_impl = async {
                        for (stream, packet) in input_context.packets().flatten() {
                            if stream.index() == video_stream_index {
                                video_playback_thread.receive_packet(packet).await;
                            }
                        }
                    }
                    .fuse()
                    .shared();

                    loop {
                        let packet_forwarder: OptionFuture<_> = if playing {
                            Some(packet_forwarder_impl.clone())
                        } else {
                            None
                        }
                        .into();

                        smol::pin!(packet_forwarder);

                        futures::select! {
                            _ = packet_forwarder => {},
                            received_command = control_receiver.recv().fuse() => {
                                match received_command {
                                    Ok(command) => {
                                        video_playback_thread.send_control_message(command).await;
                                        match command {
                                            ControlCommand::Play => {
                                                playing = true;
                                            },
                                            ControlCommand::Pause => {
                                                playing = false;
                                            }
                                        }
                                    },
                                    Err(_) => {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                })
            })?;

        let playing = true;
        playing_changed_callback(playing);

        self.control_sender.replace(control_sender);
        self.demuxer_thread.replace(demuxer_thread);
        self.playing = playing;
        self.playing_changed_callback
            .replace(Box::new(playing_changed_callback));

        Ok(())
    }

    pub fn toggle_pause_playing(&mut self) {
        if self.playing {
            self.playing = false;
            if let Some(control_sender) = &self.control_sender {
                control_sender.send_blocking(ControlCommand::Pause).unwrap();
            }
        } else {
            self.playing = true;
            if let Some(control_sender) = &self.control_sender {
                control_sender.send_blocking(ControlCommand::Play).unwrap();
            }
        }

        if let Some(playing_changed_callback) = &self.playing_changed_callback {
            playing_changed_callback(self.playing);
        }
    }
}
