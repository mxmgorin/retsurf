use crate::config::InterfaceConfig;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture, TextureCreator};
use sdl2::video::{Window, WindowContext};
use sdl2::{Sdl, VideoSubsystem};

pub struct Sdl2Renderer {
    video_subsystem: VideoSubsystem,
    texture_creator: TextureCreator<WindowContext>,
    texture: Texture,
    rect: Rect,
    pub canvas: Canvas<Window>,
}

impl Sdl2Renderer {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Self {
        let rect = Rect::new(0, 0, config.width, config.height);
        let video_subsystem = sdl.video().unwrap();
        let window = video_subsystem
            .window("Retsurf SDL2", config.width, config.height)
            .position_centered()
            .resizable()
            .build()
            .unwrap();
        let canvas = window.into_canvas().build().unwrap();
        let texture_creator = canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::RGBA8888, config.width, config.height)
            .unwrap();
        texture.set_blend_mode(sdl2::render::BlendMode::Blend);

        Self {
            video_subsystem,
            texture_creator,
            canvas,
            texture,
            rect,
        }
    }

    pub fn draw_buffer(&mut self, buffer: &[u8], config: &InterfaceConfig) {
        self.clear();
        let pitch = config.width * 4;
        self.texture.update(None, buffer, pitch as usize).unwrap();
        self.canvas
            .copy(&self.texture, None, Some(self.rect))
            .unwrap();
    }

    pub fn show(&mut self) {
        self.canvas.present();
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        if fullscreen {
            self.canvas
                .window_mut()
                .set_fullscreen(sdl2::video::FullscreenType::Desktop)
                .unwrap();
        } else {
            self.canvas
                .window_mut()
                .set_fullscreen(sdl2::video::FullscreenType::Off)
                .unwrap();
        }
        self.update_rect();
    }

    fn clear(&mut self) {
        self.canvas.set_draw_color(Color::RGB(0, 0, 0)); // black
        self.canvas.clear();
    }

    fn update_rect(&mut self) {
        let (win_width, win_height) = self.canvas.window().size();
        self.rect = Rect::new(0, 0, win_width, win_height);
    }
}
