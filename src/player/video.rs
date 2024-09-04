extern crate ffmpeg_the_third as ffmpeg;

use super::clock::StreamClock;
use super::ControlCommand;
use futures::{future::OptionFuture, FutureExt};

pub struct VideoPlaybackThread {
    control_sender: smol::channel::Sender<ControlCommand>,
    packet_sender: smol::channel::Sender<ffmpeg::codec::packet::packet::Packet>,
    receiver_thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for VideoPlaybackThread {
    fn drop(&mut self) {
        self.control_sender.close();
        if let Some(receiver_join_handle) = self.receiver_thread.take() {
            receiver_join_handle.join().unwrap();
        }
    }
}

impl VideoPlaybackThread {
    pub fn start(
        stream: &ffmpeg::format::stream::Stream,
        mut video_frame_callback: Box<dyn FnMut(&ffmpeg::util::frame::Video) + Send>,
    ) -> Result<Self, anyhow::Error> {
        let (control_sender, control_receiver) = smol::channel::unbounded::<ControlCommand>();
        let (packet_sender, packet_receiver) =
            smol::channel::bounded::<ffmpeg::codec::packet::packet::Packet>(128);

        let decoder_context = ffmpeg::codec::Context::from_parameters(stream.parameters())?;
        let mut packet_decoder = decoder_context.decoder().video()?;

        let clock = StreamClock::new(stream);

        let receiver_thread = std::thread::Builder::new()
            .name("video playback thread".into())
            .spawn(move || {
                smol::block_on(async move {
                    let packet_receiver_impl = async {
                        loop {
                            let Ok(packet) = packet_receiver.recv().await else {
                                break;
                            };

                            smol::future::yield_now().await;

                            packet_decoder.send_packet(&packet).unwrap();
                            let mut decoded_frame = ffmpeg::util::frame::Video::empty();
                            while packet_decoder.receive_frame(&mut decoded_frame).is_ok() {
                                if let Some(delay) =
                                    clock.convert_pts_to_instant(decoded_frame.pts())
                                {
                                    smol::Timer::after(delay).await;
                                }

                                video_frame_callback(&decoded_frame);
                            }
                        }
                    }
                    .fuse()
                    .shared();

                    let mut playing = true;

                    loop {
                        let packet_receiver: OptionFuture<_> = if playing {
                            Some(packet_receiver_impl.clone())
                        } else {
                            None
                        }
                        .into();

                        smol::pin!(packet_receiver);

                        futures::select! {
                            _ = packet_receiver => {},
                            received_command = control_receiver.recv().fuse() => {
                                match received_command {
                                    Ok(ControlCommand::Pause) => {
                                        playing = false;
                                    }
                                    Ok(ControlCommand::Play) => {
                                        playing = true;
                                    }
                                    Err(_) => return,
                                }
                            }
                        }
                    }
                })
            })?;

        Ok(Self {
            control_sender,
            packet_sender,
            receiver_thread: Some(receiver_thread),
        })
    }

    pub async fn receive_packet(&self, packet: ffmpeg::codec::packet::packet::Packet) -> bool {
        match self.packet_sender.send(packet).await {
            Ok(_) => true,
            Err(smol::channel::SendError(_)) => false,
        }
    }

    pub async fn send_control_message(&self, message: ControlCommand) {
        self.control_sender.send(message).await.unwrap();
    }
}
