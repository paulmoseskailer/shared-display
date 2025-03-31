#![feature(ptr_metadata)]
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_graphics::{
    geometry::Size,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyle, TextStyleBuilder},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::{
    sharable_display::DisplayPartition,
    toolkit::{App, AppImpl, DummyApp, LocalBox, SharedDisplay},
};

type DisplayType = DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>;

fn init_simulator_display() -> (SimulatorDisplay<BinaryColor>, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn flush_simulator_display(
    display: &mut SimulatorDisplay<BinaryColor>,
    window: &mut Window,
) -> bool {
    window.update(display);
    if window.events().any(|e| e == SimulatorEvent::Quit) {
        return false;
    }
    Timer::after_millis(50).await;
    true
}

struct LineApp {
    even_frame: bool,
}

impl AppImpl for LineApp {
    type Display = DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>;

    async fn update_display_impl(&mut self, display: &mut Self::Display) -> Option<Rectangle> {
        display.clear(BinaryColor::Off).await.unwrap();

        self.even_frame = !self.even_frame;
        if self.even_frame {
            return None;
        } else {
            Line::new(Point::new(0, 0), Point::new(128, 128))
                .draw_styled(&PrimitiveStyle::with_stroke(BinaryColor::On, 1), display)
                .await
                .unwrap();
            Line::new(Point::new(0, 63), Point::new(63, 0))
                .draw_styled(&PrimitiveStyle::with_stroke(BinaryColor::On, 1), display)
                .await
                .unwrap();

            return Some(Rectangle::with_corners(
                Point::new(0, 0),
                Point::new(63, 63),
            ));
        }
    }
}

struct TextApp<'a> {
    even_frame: bool,
    character_style: MonoTextStyle<'a, BinaryColor>,
    text_style: TextStyle,
}

impl<'a> AppImpl for TextApp<'a> {
    type Display = DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>;

    async fn update_display_impl(&mut self, display: &mut Self::Display) -> Option<Rectangle> {
        display.clear(BinaryColor::Off).await.unwrap();

        self.even_frame = !self.even_frame;
        if self.even_frame {
            return None;
        } else {
            Text::with_text_style(
                "hello \n world",
                Point::new(30, 20),
                self.character_style,
                self.text_style,
            )
            .draw(display)
            .await
            .unwrap();

            return Some(Rectangle::with_corners(
                Point::new(0, 0),
                Point::new(63, 63),
            ));
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let (mut display, mut window) = init_simulator_display();

    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let mut shared_display: SharedDisplay = SharedDisplay::new().await;
    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    let mut right_display = shared_display
        .new_partition(&mut display, right_rect)
        .unwrap();
    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    let mut left_display = shared_display
        .new_partition(&mut display, left_rect)
        .unwrap();

    let mut app_1 = LineApp { even_frame: true };
    let mut app_2 = TextApp {
        even_frame: false,
        character_style,
        text_style,
    };

    let mut d = DummyApp {};

    assert_eq!(core::ptr::metadata(&app_1), core::ptr::metadata(&app_2));
    assert_eq!(core::ptr::metadata(&d), core::ptr::metadata(&app_2));

    let apps: &mut [&mut dyn App<Display = DisplayType>] = &mut [&mut app_1, &mut app_2];

    let displays = &mut [&mut left_display, &mut right_display];

    loop {
        let mut total_updated_area: Option<Rectangle> = None;
        for (i, app) in apps.into_iter().enumerate() {
            let boxed_fut = app.update_display(&mut displays[i]);
        }
        if !flush_simulator_display(&mut display, &mut window).await {
            break;
        }
        Timer::after_millis(500).await;
    }
}
