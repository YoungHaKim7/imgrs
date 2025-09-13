use clap::Parser;

#[derive(Parser)]
#[command(name = "imgcat")]
#[command(about = "Display images and gifs in your terminal emulator")]
#[command(version)]
pub struct Args {
    /// Input image file (if not provided, reads from stdin)
    pub input: Option<String>,

    /// Interpolation method
    #[arg(long, default_value = "lanczos")]
    pub interpolation: String,

    /// Hide exit message
    #[arg(long, default_value = "false")]
    pub silent: bool,

    /// Image resize type
    #[arg(long, default_value = "fit")]
    pub resize_type: String,

    /// Offset from the top of the terminal to start rendering the image
    #[arg(long, default_value = "8")]
    pub top_offset: usize,
}
