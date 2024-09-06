slint::include_modules!();

extern crate ffmpeg_the_third as ffmpeg;

mod player;

use std::{cell::RefCell, rc::Rc};

use player::Player;
use slint_ffmpeg::{rgba_rescaler_for_frame, video_frame_to_pixel_buffer, Rescaler};

fn main() -> Result<(), slint::PlatformError> {
    let app = App::new()?;
    let app_strong = app.clone_strong();

    let player = Rc::new(RefCell::new(Player::new()));

    app_strong.on_open_file({
        let player_clone = Rc::clone(&player);

        move || {
            let pick_file = rfd::FileDialog::new().pick_file();
            let mut to_rgba_rescaler: Option<Rescaler> = None;

            if let Some(path) = pick_file {
                player_clone
                    .borrow_mut()
                    .start(
                        path,
                        // i think i need to hide these 2 Fn stuffs,
                        // i should only need the path to start the player
                        {
                            let app_weak = app.as_weak();
                            move |new_frame| {
                                let rebuild_rescaler =
                                    to_rgba_rescaler.as_ref().map_or(true, |existing_rescaler| {
                                        existing_rescaler.input().format != new_frame.format()
                                    });

                                if rebuild_rescaler {
                                    to_rgba_rescaler = Some(rgba_rescaler_for_frame(new_frame));
                                }

                                let rescaler = to_rgba_rescaler.as_mut().unwrap();

                                let mut rgb_frame = ffmpeg::util::frame::Video::empty();
                                rescaler.run(new_frame, &mut rgb_frame).unwrap();

                                let pixel_buffer = video_frame_to_pixel_buffer(&rgb_frame);

                                app_weak
                                    .upgrade_in_event_loop(|app| {
                                        app.set_video_frame(slint::Image::from_rgb8(pixel_buffer))
                                    })
                                    .unwrap();
                            }
                        },
                        {
                            let app_weak = app.as_weak();
                            move |playing| {
                                app_weak
                                    .upgrade_in_event_loop(move |app| app.set_playing(playing))
                                    .unwrap();
                            }
                        },
                    )
                    .unwrap();
            }
        }
    });

    app_strong.on_toggle_pause_play({
        let player_clone = Rc::clone(&player);
        move || {
            player_clone.borrow_mut().toggle_pause_playing();
        }
    });

    app_strong.run()
}
