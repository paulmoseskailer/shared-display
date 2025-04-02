use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Instant, Timer};
use embedded_graphics::{
    geometry::Size,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, StyledDrawable},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::{
    sharable_display::DisplayPartition,
    toolkit::{flush_loop, FlushResult, SharedDisplay},
};
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

#[embassy_executor::task(pool_size = 4)]
async fn draw_cross_recursive(
    spawner: &'static Spawner,
    recursion_level: u8,
    mut display: DisplayPartition<BinaryColor, DisplayType>,
) -> () {
    let max_x: i32 = (display.bounding_box().size.width - 1).try_into().unwrap();
    let max_y: i32 = (display.bounding_box().size.height - 1).try_into().unwrap();
    let start = Instant::now();

    loop {
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
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        if recursion_level > 0 && Instant::now().duration_since(start).as_secs() > 1 {
            break;
        }
    }
    // recursive case
    let (left_display, right_display) = SHARED_DISPLAY
        .lock()
        .await
        .as_mut()
        .unwrap()
        .split_existing_unchecked(display.partition)
        .await
        .unwrap();
    spawner.must_spawn(draw_cross_recursive(
        spawner,
        recursion_level - 1,
        left_display,
    ));
    spawner.must_spawn(draw_cross_recursive(
        spawner,
        recursion_level - 1,
        right_display,
    ));
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();

    let shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display).await;
    {
        let mut guard = SHARED_DISPLAY.lock().await;
        *guard = Some(shared_display);
    }
    let spawner_ref: &'static Spawner = SPAWNER.init(spawner);

    let (left_display, right_display) = SHARED_DISPLAY
        .lock()
        .await
        .as_mut()
        .unwrap()
        .split_vertically()
        .await
        .unwrap();

    spawner.must_spawn(draw_cross_recursive(spawner_ref, 0, left_display));
    spawner.must_spawn(draw_cross_recursive(spawner_ref, 1, right_display));

    flush_loop(&SHARED_DISPLAY, async |d| {
        window.update(d);
        if window.events().any(|e| e == SimulatorEvent::Quit) {
            return FlushResult::Abort;
        }
        FlushResult::Continue
    })
    .await;
}
