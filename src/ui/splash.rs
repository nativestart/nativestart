use std::time::Duration;
use std::sync::{mpsc, Arc};
use std::sync::mpsc::Receiver;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{self, BufRead};
use std::path::Path;
use std::path::PathBuf;
use std::collections::HashMap;
use std::fs::File;

use winit::event_loop::EventLoop;
use image::{DynamicImage};

use minifb::{Scale, Window, WindowOptions};
use raqote::{DrawOptions, DrawTarget, Image, PathBuilder, Point, SolidSource, Source, Transform, ExtendMode, FilterMode};
use font_kit::loaders::default::Font;
use euclid::vec2;

use crate::errors::*;
use crate::ui::{Message, MAX_DOWNLOAD_PROGRESS};

macro_rules! parse {
    ( $cmd:expr, $( $x:expr ),* ) => {
        {
            let mut index = 0;
            $(
                index = index + 1;
                $x = $cmd[index].parse::<>().expect(
                    format!("parameter {} of command {} has wrong type", index, $cmd[0]).as_str()
                );
            )*
        }
    };
}

pub struct Splash {
    app_name: &'static str,
    version: String,
    image_path: PathBuf,
}

struct SplashImpl {
    width: usize,
    height: usize,
    background: Vec<Vec<String>>,
    progress: Vec<Vec<String>>
}

struct DrawContext {
    scale: f64,
    fill: (u8, u8, u8, u8),
    text_font: Option<Font>,
    text_size: f32,
    text_align: f32,

    basedir: PathBuf,
    images: HashMap<String, (u32, u32, Vec<u32>)>,
    placeholders: HashMap<String, String>,

    draw_target: DrawTarget
}

impl Splash {
    pub fn new(app_name: &'static str, version: String, image_dir: PathBuf) -> Splash {
        return Splash {
            app_name,
            version,
            image_path: image_dir,
        };
    }
    pub fn show_and_await_termination(&mut self, rx: Receiver<Message>) -> Result<()> {
        let (screen_width, screen_height, screen_scale, img_scale, dpi) = Splash::get_screen_size();

        let splash = Splash::parse_splash(&self.image_path);
        let window_width = (splash.width as f64 * screen_scale) as usize;
        let window_height = (splash.height as f64 * screen_scale) as usize;
        let img_width = (splash.width as f64 * img_scale) as usize;
        let img_height = (splash.height as f64 * img_scale) as usize;

        let mut window = Window::new(
            self.app_name,
            window_width,
            window_height,
            WindowOptions {
                borderless: true,
                title: false,
                resize: false,
                scale: Scale::X1,
                none: true,
                ..WindowOptions::default()
            },
        ).expect("failed to create window");
        window.set_position(((screen_width - window_width as i32) / 2) as isize, ((screen_height - window_height as i32) / 2) as isize);

        let mut placeholders = HashMap::new();
        placeholders.insert(String::from("dpi"), dpi);
        placeholders.insert(String::from("version"), String::from(&self.version));

        let mut draw_context = DrawContext {
            scale: img_scale,
            fill: (0, 0, 0, 255),
            text_font: None,
            text_size: 12.0,
            text_align: 0.0,
            basedir: self.image_path.clone(),
            images: HashMap::new(),
            placeholders,

            draw_target: DrawTarget::new(img_width as i32, img_height as i32)
        };

        for tokens in &splash.background {
            draw_context = Splash::execute_command(tokens, draw_context);
        }

        let mut cur_progress: Option<Arc<AtomicUsize>> = None;
        let mut status = "";
        let mut exit_loop = false;
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));
        loop {
            draw_context.placeholders.insert(String::from("status"), String::from(status));
            for tokens in &splash.background {
                draw_context = Splash::execute_command(tokens, draw_context);
            }

            if let Some(progress) = &cur_progress {
                let progress = progress.load(Ordering::SeqCst) as f64 / MAX_DOWNLOAD_PROGRESS as f64;
                draw_context.placeholders.insert(String::from("progress"),progress.to_string());
                for tokens in &splash.progress {
                    draw_context = Splash::execute_command(tokens, draw_context);
                }
            }

            window.update_with_buffer(draw_context.draw_target.get_data(), img_width, img_height).unwrap();

            if exit_loop {
                // exit loop after UI has been redrawn
                break;
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(Message::Error(val)) => {
                    crate::show_error_message(&self.app_name, val, true);
                },
                Ok(Message::Downloading(val)) => {
                    status = "Downloading";
                    cur_progress = Some(val);
                },
                Ok(Message::FilesReady) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    status = "Starting";
                    cur_progress = None;
                    exit_loop = true;
                },
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => ()
            }
        }

        Splash::await_termination(&self.app_name, rx, window);

        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    fn await_termination(app_name: &'static str, rx: Receiver<Message>, window: Window) {
        let mut win = Some(window);
        loop {
            match rx.recv() {
                Ok(Message::ApplicationUiVisible)  => {
                    drop(win); // close window
                    win = None;
                },
                Ok(Message::Error(val)) => {
                    crate::show_error_message(app_name, val, true);
                },
                Ok(Message::ApplicationTerminated) | Err(mpsc::RecvError) => {
                    break;
                },
                Ok(_) => ()
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn await_termination(app_name: &'static str, rx: Receiver<Message>, window: Window) {
        use std::thread;
        use std::process::exit;
        use send_wrapper::SendWrapper;
        use cocoa::appkit::{NSApp, NSApplication};
        use dispatch::Queue;

        let wrapped_window = SendWrapper::new(window);
        let (sender, receiver) = mpsc::channel();
        sender.send(wrapped_window).unwrap();

        thread::spawn(move|| {
            match rx.recv() {
                Ok(Message::ApplicationUiVisible) | Err(mpsc::RecvError) => {
                    Queue::main().sync_exec(move || {
                        let received_window = receiver.recv().unwrap();
                        drop(received_window.take()); // close window
                    });
                },
                Ok(Message::Error(val)) => {
                    Queue::main().sync_exec(move || {
                        crate::show_error_message(app_name, val.clone(), true);
                    });
                },
                Ok(Message::ApplicationTerminated) | Err(_) => {
                    exit(0)
                },
                Ok(_) => ()
            }

            loop {
                match rx.recv() {
                    Ok(Message::Error(val)) => {
                        Queue::main().sync_exec(move || {
                            crate::show_error_message(app_name, val.clone(), true);
                        });
                    },
                    Ok(Message::ApplicationTerminated) | Err(_) => {
                        exit(0);
                    },
                    Ok(_) => ()
                }
            }
        });

        unsafe {
            NSApp().run();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn get_screen_size() -> (i32, i32, f64, f64, String) {
        let events_loop = EventLoop::new();
        let monitor = events_loop.primary_monitor().unwrap();
        let factor = monitor.scale_factor();
        let width = monitor.size().width as i32;
        let height = monitor.size().height as i32;
        let (factor, dpi) = Splash::map_scale(factor);

        return (width, height, factor, factor, dpi);
    }

    #[cfg(target_os = "macos")]
    fn get_screen_size() -> (i32, i32, f64, f64, String) {
        use winit::dpi::LogicalSize;

        let events_loop = EventLoop::new();
        let monitor = events_loop.primary_monitor().unwrap();
        let factor = monitor.scale_factor();

        // Dimensions returned by winit are converted to physical size,
        // therefore we need to convert them back to logical size
        let dimensions: LogicalSize<i32> = monitor.size().to_logical(factor);
        let width = dimensions.width;
        let height = dimensions.height;

        let (factor, dpi) = Splash::map_scale(factor);

        // MacOS uses logical coordinates for window size and positioning, not physical
        return (width, height, 1.0, factor, dpi);
    }

    fn map_scale(scale: f64) -> (f64, String) {
        return if scale < 1.25 {
            (1.0, String::from("mdpi"))
        } else if scale < 1.75 {
            (1.5, String::from("hdpi"))
        } else {
            (2.0, String::from("xhdpi"))
        }
    }


    fn parse_splash(splash_dir: &PathBuf) -> SplashImpl {
        let mut width: usize = 0;
        let mut height: usize = 0;
        let mut background: Vec<Vec<String>> = Vec::new();
        let mut progress: Vec<Vec<String>> = Vec::new();
        let mut is_background = true;

        let mut path = splash_dir.clone();
        path.push("splash");
        if let Ok(lines) = Splash::read_lines(path) {
            for line in lines {
                if let Ok(ln) = line {
                    match ln.as_str() {
                        "[background]" => {
                            is_background = true;
                        }
                        "[progress]" => {
                            is_background = false;
                        }
                        _ => {
                            let tokens = ln
                                .split_whitespace()
                                .map(|token| token.to_string())
                                .collect::<Vec<String>>();
                            if tokens.len() > 0 {
                                if tokens[0].eq("splash") {
                                    parse!(tokens, width, height);
                                } else {
                                    if is_background {
                                        background.push(tokens);
                                    } else {
                                        progress.push(tokens);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return SplashImpl {
            width,
            height,
            background,
            progress
        }
    }

    fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
        where P: AsRef<Path>, {
        let file = File::open(filename)?;
        Ok(io::BufReader::new(file).lines())
    }

    fn execute_command(tokens: &Vec<String>, mut draw_context: DrawContext) -> DrawContext {
        match tokens[0].as_str() {
            "image" => {
                let mut path: String;
                let x: String;
                let y: String;
                let w: String;
                let h: String;
                if tokens.len() == 6 {
                    parse!(tokens, path, x, y, w, h);
                } else {
                    parse!(tokens, path, x, y);
                    w = String::from("-1");
                    h = String::from("-1");
                }
                path = draw_context.eval_text(path);
                let x = draw_context.eval_num(x) * draw_context.scale;
                let y = draw_context.eval_num(y) * draw_context.scale;
                let w = draw_context.eval_num(w) * draw_context.scale;
                let h = draw_context.eval_num(h) * draw_context.scale;

                if !draw_context.images.contains_key(path.as_str()) {
                    let mut path_buffer = draw_context.basedir.clone();
                    path_buffer.push(path.as_str());
                    let img = image::open(path_buffer).unwrap();
                    let img = match img {
                        DynamicImage::ImageRgba8(img) => img,
                        img => img.to_rgba8()
                    };
                    let width = img.dimensions().0;
                    let height = img.dimensions().1;
                    let mut buf: Vec<u32> = vec![0; (width * height) as usize];
                    let mut i = 0;
                    for p in img.pixels() {
                        let alpha = p.0[3] as u32;
                        let r = (p.0[0] as u32 * alpha) >> 8;
                        let g = (p.0[1] as u32 * alpha) >> 8;
                        let b = (p.0[2] as u32 * alpha) >> 8;
                        buf[i] = alpha << 24 | r << 16 | g << 8 | b;
                        i = i + 1;
                    }
                    draw_context.images.insert(path.clone(), (width, height, buf));
                }

                let value = draw_context.images.get(path.as_str()).unwrap();
                let img = &Image {
                    width: value.0 as i32,
                    height: value.1 as i32,
                    data: &value.2,
                };

                if w > 0.0 && h > 0.0 {
                    let mut pb = PathBuilder::new();
                    pb.rect(x as f32, y as f32, w as f32, h as f32);
                    let ts = Transform::identity().then_translate(vec2(-x as f32, -y as f32)).inverse().unwrap();

                    let source = Source::Image(*img,
                                               ExtendMode::Pad,
                                               FilterMode::Nearest,
                                               ts);
                    draw_context.draw_target.fill(&pb.finish(), &source, &DrawOptions::default());
                } else {
                    draw_context.draw_target.draw_image_at(x as f32, y as f32,img, &DrawOptions::default());
                }
            }
            "textfont" => {
                let mut path_buffer = draw_context.basedir.clone();
                path_buffer.push(tokens[1].clone());
                draw_context.text_font = Some(
                    Font::from_path(path_buffer, 0).expect("failed to load font"),
                );
            }
            "textsize" => {
                parse!(tokens, draw_context.text_size);
            }
            "textalign" => {
                let align: String;
                parse!(tokens, align);
                if align == "start" || align == "left" {
                    draw_context.text_align = 0.0;
                } else if align == "end" || align == "right" {
                    draw_context.text_align = 1.0;
                } else if align == "center" {
                    draw_context.text_align = 0.5;
                }
            }
            "fill" => {
                let r: u8;
                let g: u8;
                let b: u8;
                parse!(tokens, r, g, b);
                draw_context.fill = (r, g, b, 255);
            }
            "filltext" => {
                let source = Source::Solid(SolidSource {
                    r: draw_context.fill.0,
                    g: draw_context.fill.1,
                    b: draw_context.fill.2,
                    a: 255,
                });

                let x: String;
                let y: String;
                parse!(tokens, x, y);
                let x = draw_context.eval_num(x) * draw_context.scale;
                let y = draw_context.eval_num(y) * draw_context.scale;
                let text = draw_context.eval_text(tokens[3..].join(" "));

                let pointsize = draw_context.text_size * draw_context.scale as f32;
                let font = &draw_context.text_font.clone().unwrap();

                let mut width = 0.0;
                for c in text.chars() {
                    let id = font.glyph_for_char(c).unwrap();
                    width = width + font.advance(id).unwrap().x() as f32 * pointsize / 24. / 96.;
                }

                draw_context.draw_target.draw_text(
                    &draw_context.text_font
                        .clone()
                        .expect("text font must be given before text is drawn"),
                    pointsize,
                    text.as_str(),
                    Point::new(x as f32 - width * draw_context.text_align, y as f32),
                    &source,
                    &DrawOptions {
                        alpha: draw_context.fill.3 as f32 / 255.0,
                        ..DrawOptions::default()
                    },
                );
            }
            _ => {

            }
        }
        return draw_context;
    }
}

impl DrawContext {
    fn eval_text(&self, text: String) -> String {
        let mut text = text.clone();
        for (key, value) in &self.placeholders {
            text = text.replace(format!("${{{}}}", key).as_str(), value);
        }
        return text;
    }
    fn eval_num(&self, text: String) -> f64 {
        return meval::eval_str(self.eval_text(text)).unwrap();
    }
}
