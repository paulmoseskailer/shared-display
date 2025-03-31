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
use shared_display::{sharable_display::DisplayPartition, toolkit::SharedDisplay};
use static_cell::StaticCell;

type MySimDisplay = SimulatorDisplay<BinaryColor>;
static SPAWNER: StaticCell<Spawner> = StaticCell::new();
static SHARED_DISPLAY: Mutex<CriticalSectionRawMutex, Option<SharedDisplay<MySimDisplay>>> =
    Mutex::new(None);

fn init_simulator_display() -> (MySimDisplay, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

#[embassy_executor::task(pool_size = 10)]
async fn draw_cross_recursive(
    spawner: &'static Spawner,
    recursion_level: u8,
    mut display: DisplayPartition<BinaryColor, MySimDisplay>,
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
        Timer::after_millis(500).await;
        Line::new(Point::new(0, max_y), Point::new(max_x, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(300).await;

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

async fn flush_simulator_display(display: &mut MySimDisplay, window: &mut Window) -> bool {
    window.update(display);
    if window.events().any(|e| e == SimulatorEvent::Quit) {
        return false;
    }
    true
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();
    let shared_display: SharedDisplay<MySimDisplay> = SharedDisplay::new(display).await;
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
        .unwrap();

    spawner.must_spawn(draw_cross_recursive(spawner_ref, 1, left_display));
    spawner.must_spawn(draw_cross_recursive(spawner_ref, 2, right_display));

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
