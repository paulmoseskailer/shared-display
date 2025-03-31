use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embedded_graphics::{
    geometry::Size,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::{sharable_display::DisplayPartition, toolkit::SharedDisplay};
use static_cell::StaticCell;

type DisplayType = SimulatorDisplay<BinaryColor>;
static SPAWNER: StaticCell<Spawner> = StaticCell::new();
static SHARED_DISPLAY: Mutex<CriticalSectionRawMutex, Option<SharedDisplay<DisplayType>>> =
    Mutex::new(None);

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

#[embassy_executor::task]
async fn print_hello(mut display: DisplayPartition<BinaryColor, DisplayType>) -> () {
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

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
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;
    }
}
#[embassy_executor::task]
async fn draw_line(mut display: DisplayPartition<BinaryColor, DisplayType>) -> () {
    loop {
        Line::new(Point::new(0, 0), Point::new(128, 128))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        Line::new(Point::new(0, 63), Point::new(63, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;
    }
}

async fn flush_simulator_display(display: &mut DisplayType, window: &mut Window) -> bool {
    window.update(display);
    if window.events().any(|e| e == SimulatorEvent::Quit) {
        return false;
    }
    true
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();
    let shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display).await;
    {
        let mut guard = SHARED_DISPLAY.lock().await;
        *guard = Some(shared_display);
    }
    let _spawner_ref: &'static Spawner = SPAWNER.init(spawner);

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    let right_display = SHARED_DISPLAY
        .lock()
        .await
        .as_mut()
        .unwrap()
        .new_partition(right_rect)
        .unwrap();
    spawner.must_spawn(draw_line(right_display));

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    let left_display = SHARED_DISPLAY
        .lock()
        .await
        .as_mut()
        .unwrap()
        .new_partition(left_rect)
        .unwrap();
    spawner.must_spawn(print_hello(left_display));

    loop {
        if !flush_simulator_display(
            &mut SHARED_DISPLAY.lock().await.as_mut().unwrap().real_display,
            &mut window,
        )
        .await
        {
            break;
        }
        Timer::after_millis(100).await;
    }
}
