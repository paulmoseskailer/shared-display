use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use embedded_graphics::{
    geometry::Size,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::toolkit::{self, FlushResult, SharedDisplay};
use shared_display_core::DisplayPartition;
use static_cell::StaticCell;

type DisplayType = SimulatorDisplay<BinaryColor>;

static SPAWNER: StaticCell<Spawner> = StaticCell::new();

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn text_app(mut display: DisplayPartition<BinaryColor, DisplayType>) -> () {
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let start = Instant::now();
    loop {
        Text::with_text_style(
            "hello \n world",
            Point::new(30, 20),
            character_style,
            text_style,
        )
        .draw(&mut display)
        .await
        .unwrap();
        Timer::after_millis(200).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        if Instant::now().duration_since(start).as_secs() > 2 {
            break;
        }
    }
}

async fn line_app(mut display: DisplayPartition<BinaryColor, DisplayType>) -> () {
    loop {
        let max_x: i32 = (display.area.size.width - 1).try_into().unwrap();
        let max_y: i32 = (display.area.size.height - 1).try_into().unwrap();
        Line::new(Point::new(0, 0), Point::new(max_x, max_y))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(200).await;
        Line::new(Point::new(0, max_y), Point::new(max_x, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(200).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        match toolkit::EVENTS.try_receive() {
            Err(_) => continue,
            Ok(event) => match event {
                toolkit::ResizeEvent::AppClosed(area) => display.envelope(&area),
            },
        };
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let spawner = SPAWNER.init(spawner);

    let (display, mut window) = init_simulator_display();
    let mut shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display).await;

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    shared_display
        .launch_new_app(spawner, line_app, right_rect)
        .await
        .unwrap();

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    shared_display
        .launch_new_app(spawner, text_app, left_rect)
        .await
        .unwrap();

    shared_display
        .flush_loop(async |d| {
            window.update(d);
            if window.events().any(|e| e == SimulatorEvent::Quit) {
                return FlushResult::Abort;
            }
            FlushResult::Continue
        })
        .await;
}
