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
    toolkit::{update_all_apps, App, AppImpl, SharedDisplay},
};

type DisplayType = SimulatorDisplay<BinaryColor>;

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        DisplayType::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn flush_simulator_display(display: &mut DisplayType, window: &mut Window) -> bool {
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
    type Display = DisplayPartition<BinaryColor, DisplayType>;

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
    type Display = DisplayPartition<BinaryColor, DisplayType>;

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

    let mut shared_display: SharedDisplay = SharedDisplay::new().await;

    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let app_1 = LineApp { even_frame: true };
    let app_2 = TextApp {
        even_frame: false,
        character_style,
        text_style,
    };

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    let mut right_display = shared_display
        .new_partition(&mut display, right_rect)
        .unwrap();

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    let mut left_display = shared_display
        .new_partition(&mut display, left_rect)
        .unwrap();

    let apps: &mut [Box<dyn App<Display = DisplayPartition<BinaryColor, DisplayType>>>] =
        &mut [Box::new(app_1), Box::new(app_2)];

    loop {
        let total_updated_area =
            update_all_apps(apps, &mut [&mut left_display, &mut right_display]).await;
        if total_updated_area.is_some() {
            if !flush_simulator_display(&mut display, &mut window).await {
                break;
            }
        }
        Timer::after_millis(500).await;
    }
}
