use anyhow::{Context, Result};
use clap::Parser;
use image::Pixel;
use image::{DynamicImage, GenericImageView, ImageFormat};
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

mod cli;
mod terminal;

use cli::Args;
use terminal::{disable_echo, get_terminal_size, is_terminal};

// Constants
const RESIZE_OFFSET_Y: usize = 8;
const RESIZE_FACTOR_Y: usize = 2;
const RESIZE_FACTOR_X: usize = 1;
const DEFAULT_TERM_COLS: usize = 80;
const DEFAULT_TERM_ROWS: usize = 24;
const FPS: u64 = 15;
const NUM_ADDITIONAL_LINES: usize = 2;

// ANSI escape codes
const ANSI_CURSOR_UP: &str = "\x1B[{}A";
const ANSI_CURSOR_HIDE: &str = "\x1B[?25l";
const ANSI_CURSOR_SHOW: &str = "\x1B[?25h";
const ANSI_BG_TRANSPARENT_COLOR: &str = "\x1b[0;39;49m";
const ANSI_BG_RGB_COLOR: &str = "\x1b[48;2;{};{};{}m";
const ANSI_FG_TRANSPARENT_COLOR: &str = "\x1b[0m ";
const ANSI_FG_RGB_COLOR: &str = "\x1b[38;2;{};{};{}m▄";
const ANSI_RESET: &str = "\x1b[0m";

#[derive(Clone)]
struct ImageFrame {
    image: DynamicImage,
}

impl ImageFrame {
    fn new(image: DynamicImage) -> Self {
        Self { image }
    }

    fn get_pixel_rgba(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let pixel = self.image.get_pixel(x, y);
        let rgba = pixel.to_rgba();
        (rgba[0], rgba[1], rgba[2], rgba[3])
    }

    fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }
}

fn read_input(input: Option<String>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();

    if let Some(path) = input {
        Ok(std::fs::read(&path).with_context(|| format!("Failed to read file: {}", path))?)
    } else {
        io::stdin()
            .read_to_end(&mut buf)
            .context("Failed to read from stdin")?;
        Ok(buf)
    }
}

fn decode_image(buf: &[u8]) -> Result<Vec<ImageFrame>> {
    // Try to decode as different formats
    if let Ok(format) = image::guess_format(buf) {
        match format {
            ImageFormat::Gif => decode_gif(buf),
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp => decode_static_image(buf),
            _ => decode_static_image(buf),
        }
    } else {
        // Try as static image
        decode_static_image(buf)
    }
}

fn decode_gif(buf: &[u8]) -> Result<Vec<ImageFrame>> {
    let decoder = gif::DecodeOptions::new();
    let mut decoder = decoder.read_info(buf)?;

    let mut frames = Vec::new();

    while let Some(frame) = decoder.read_next_frame()? {
        let img = image::RgbaImage::from_raw(
            frame.width as u32,
            frame.height as u32,
            frame.buffer.to_vec(),
        )
        .context("Failed to create image from GIF frame")?;

        frames.push(ImageFrame::new(DynamicImage::ImageRgba8(img)));
    }

    if frames.is_empty() {
        anyhow::bail!("No frames found in GIF");
    }

    Ok(frames)
}

fn decode_static_image(buf: &[u8]) -> Result<Vec<ImageFrame>> {
    let img = image::load_from_memory(buf).context("Failed to decode image")?;

    let (width, height) = img.dimensions();
    if width < 2 || height < 2 {
        anyhow::bail!("The input image is too small");
    }

    Ok(vec![ImageFrame::new(img)])
}

fn scale_frames(frames: Vec<ImageFrame>) -> Result<Vec<ImageFrame>> {
    let (cols, rows) = if is_terminal() {
        get_terminal_size().unwrap_or((DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS))
    } else {
        (DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)
    };

    let w = cols * RESIZE_FACTOR_X;
    let h = (rows - RESIZE_OFFSET_Y) * RESIZE_FACTOR_Y;

    let mut scaled_frames = Vec::with_capacity(frames.len());

    for frame in frames {
        let (orig_width, orig_height) = frame.dimensions();

        // Calculate new dimensions maintaining aspect ratio
        let aspect_ratio = orig_width as f32 / orig_height as f32;
        let target_aspect_ratio = w as f32 / h as f32;

        let (new_width, new_height) = if aspect_ratio > target_aspect_ratio {
            (w as u32, (w as f32 / aspect_ratio) as u32)
        } else {
            ((h as f32 * aspect_ratio) as u32, h as u32)
        };

        let scaled_img =
            frame
                .image
                .resize(new_width, new_height, image::imageops::FilterType::Lanczos3);
        scaled_frames.push(ImageFrame::new(scaled_img));
    }

    Ok(scaled_frames)
}

fn escape_frames(frames: Vec<ImageFrame>) -> Vec<Vec<String>> {
    let mut escaped = Vec::with_capacity(frames.len());

    for frame in frames {
        let (width, height) = frame.dimensions();
        let max_y = height - (height % 2);
        let max_x = width;

        let (tx, rx) = mpsc::channel();

        // Process each pair of rows in parallel
        for y in (0..max_y).step_by(2) {
            let tx = tx.clone();
            let frame = frame.clone();

            thread::spawn(move || {
                let mut line = String::new();

                for x in 0..max_x {
                    // Upper pixel (background)
                    let (r, g, b, a) = frame.get_pixel_rgba(x, y);
                    if a < 128 {
                        line.push_str(ANSI_BG_TRANSPARENT_COLOR);
                    } else {
                        line.push_str(&format!("{} {} {} {}", ANSI_BG_RGB_COLOR, r, g, b));
                    }

                    // Lower pixel (foreground)
                    let (r, g, b, a) = frame.get_pixel_rgba(x, y + 1);
                    if a < 128 {
                        line.push_str(ANSI_FG_TRANSPARENT_COLOR);
                    } else {
                        line.push_str(&format!("{} {} {} {}", ANSI_FG_RGB_COLOR, r, g, b));
                    }
                }

                line.push_str(ANSI_RESET);
                line.push('\n');

                tx.send((y / 2, line)).unwrap();
            });
        }

        drop(tx); // Close the sender

        let mut lines = vec![String::new(); (max_y / 2) as usize];
        while let Ok((idx, line)) = rx.recv() {
            lines[idx as usize] = line;
        }

        escaped.push(lines);
    }

    escaped
}

fn print_frames(frames: Vec<Vec<String>>, silent: bool) -> Result<()> {
    let _term_state = if is_terminal() {
        Some(disable_echo())
    } else {
        None
    };

    print!("{}", ANSI_CURSOR_HIDE);
    print!("\n");

    let frame_count = frames.len();

    if frame_count == 1 {
        for line in &frames[0] {
            print!("{}", line);
        }
    } else {
        // Setup signal handling for Ctrl+C
        let playing = Arc::new(AtomicBool::new(true));
        let p = playing.clone();
        ctrlc::set_handler(move || {
            p.store(false, Ordering::SeqCst);
        })?;

        let frame_duration = Duration::from_millis(1000 / FPS);
        let h = frames[0].len() + if silent { 0 } else { NUM_ADDITIONAL_LINES };

        let mut i = 0;
        while playing.load(Ordering::SeqCst) {
            if i != 0 {
                print!("{}", format!("{} {}", ANSI_CURSOR_UP, h));
            }

            for line in &frames[i % frame_count] {
                print!("{}", line);
            }

            if !silent {
                print!("\npress `ctrl c` to exit\n");
            }

            thread::sleep(frame_duration);
            i += 1;
        }
    }

    print!("{}", ANSI_RESET);
    print!("{}", ANSI_CURSOR_SHOW);

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    let input_data = read_input(args.input)?;
    let frames = decode_image(&input_data)?;
    let scaled_frames = scale_frames(frames)?;
    let escaped_frames = escape_frames(scaled_frames);

    print_frames(escaped_frames, args.silent)?;

    Ok(())
}
