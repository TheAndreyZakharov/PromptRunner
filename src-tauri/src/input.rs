use crate::error::{AppError, AppResult};
use crate::model::Zone;
#[cfg(target_os = "macos")]
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Mouse, Settings};
#[cfg(not(target_os = "macos"))]
use enigo::{Key, Keyboard};
use std::thread;
use std::time::Duration;

pub fn initialize() {
    // Keep screenshot pixels, zones and Windows absolute mouse coordinates in
    // the same DPI space. The call is intentionally best-effort because a
    // packaged manifest may already declare the process DPI awareness.
    #[cfg(target_os = "windows")]
    {
        let _ = enigo::set_dpi_awareness();
    }
}

pub fn current_pointer() -> AppResult<(i32, i32)> {
    let enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::Input(e.to_string()))?;
    enigo.location().map_err(|e| AppError::Input(e.to_string()))
}

pub fn click_zone(zone: &Zone, attempt: u8, origin: (i32, i32)) -> AppResult<()> {
    if !zone.is_valid() {
        return Err(AppError::message(format!(
            "Некорректная зона {}",
            zone.name
        )));
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::Input(e.to_string()))?;
    let point = sample_point(zone, attempt, origin);
    let start = enigo.location().unwrap_or(point);
    move_smooth(&mut enigo, start, point, attempt)?;
    thread::sleep(Duration::from_millis(90));
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| AppError::Input(e.to_string()))
}

pub fn move_cursor_away(
    zone: &Zone,
    origin: (i32, i32),
    screen_width: u32,
    screen_height: u32,
) -> AppResult<()> {
    if !zone.is_valid() {
        return Err(AppError::message(format!(
            "Некорректная зона {}",
            zone.name
        )));
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::Input(e.to_string()))?;
    let start = enigo.location().unwrap_or(origin);
    let point = safe_point_outside(zone, origin, screen_width, screen_height);
    move_smooth(&mut enigo, start, point, 7)?;
    thread::sleep(Duration::from_millis(50));
    Ok(())
}

/// Move the real system cursor into the calibrated response area and scroll
/// the target chat down. Keeping the move and wheel event in one Enigo
/// instance is important on Windows, where the wheel is sent to the window
/// currently under the cursor.
pub fn scroll_down(zone: &Zone, attempt: u8, origin: (i32, i32)) -> AppResult<()> {
    if !zone.is_valid() {
        return Err(AppError::message(format!(
            "Некорректная зона {}",
            zone.name
        )));
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::Input(e.to_string()))?;
    let point = sample_point(zone, attempt, origin);
    let start = enigo.location().unwrap_or(point);
    move_smooth(&mut enigo, start, point, attempt)?;
    thread::sleep(Duration::from_millis(35));
    enigo
        .scroll(5, Axis::Vertical)
        .map_err(|e| AppError::Input(e.to_string()))
}

pub fn paste_text(zone: &Zone, text: &str, origin: (i32, i32)) -> AppResult<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    clipboard
        .set_text(text)
        .map_err(|e| AppError::Clipboard(e.to_string()))?;
    click_zone(zone, 0, origin)?;
    // Даём приложению время обработать клик и принять фокус перед вставкой.
    thread::sleep(Duration::from_millis(180));
    #[cfg(target_os = "macos")]
    send_mac_shortcut(0x09)?;
    #[cfg(not(target_os = "macos"))]
    send_windows_shortcut(Key::Unicode('v'))?;
    Ok(())
}

pub fn new_chat(zone: &Zone, origin: (i32, i32)) -> AppResult<()> {
    click_zone(zone, 0, origin)?;
    #[cfg(target_os = "macos")]
    send_mac_shortcut(0x2D)?;
    #[cfg(not(target_os = "macos"))]
    send_windows_shortcut(Key::Unicode('n'))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn send_mac_shortcut(keycode: u16) -> AppResult<()> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| {
        AppError::Input("Не удалось создать источник macOS-событий клавиатуры".into())
    })?;
    let command_down = CGEvent::new_keyboard_event(source.clone(), KeyCode::COMMAND, true)
        .map_err(|_| AppError::Input("Не удалось создать событие Cmd".into()))?;
    command_down.set_flags(CGEventFlags::CGEventFlagCommand);
    command_down.post(CGEventTapLocation::Session);

    let key_down = CGEvent::new_keyboard_event(source.clone(), keycode, true)
        .map_err(|_| AppError::Input("Не удалось создать событие клавиши".into()))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::Session);
    let key_up = CGEvent::new_keyboard_event(source.clone(), keycode, false)
        .map_err(|_| AppError::Input("Не удалось создать отпускание клавиши".into()))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::Session);
    let command_up = CGEvent::new_keyboard_event(source, KeyCode::COMMAND, false)
        .map_err(|_| AppError::Input("Не удалось отпустить Cmd".into()))?;
    command_up.post(CGEventTapLocation::Session);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn send_windows_shortcut(key: Key) -> AppResult<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::Input(e.to_string()))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| AppError::Input(e.to_string()))?;
    enigo
        .key(key, Direction::Click)
        .map_err(|e| AppError::Input(e.to_string()))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| AppError::Input(e.to_string()))?;
    Ok(())
}

pub fn sample_point(zone: &Zone, attempt: u8, origin: (i32, i32)) -> (i32, i32) {
    let slots = [(25_u32, 25_u32), (75, 25), (25, 75), (75, 75), (50, 50)];
    let (px, py) = slots[(attempt as usize) % slots.len()];
    (
        origin.0 + zone.x + ((zone.width.saturating_mul(px)) / 100) as i32,
        origin.1 + zone.y + ((zone.height.saturating_mul(py)) / 100) as i32,
    )
}

pub fn safe_point_outside(
    zone: &Zone,
    origin: (i32, i32),
    screen_width: u32,
    screen_height: u32,
) -> (i32, i32) {
    let left = origin.0 + zone.x;
    let top = origin.1 + zone.y;
    let right = left + zone.width as i32;
    let bottom = top + zone.height as i32;
    let margin = 24_i32;
    let points = [
        (origin.0 + margin, origin.1 + margin),
        (
            origin.0 + screen_width.saturating_sub(margin as u32) as i32,
            origin.1 + margin,
        ),
        (
            origin.0 + margin,
            origin.1 + screen_height.saturating_sub(margin as u32) as i32,
        ),
        (
            origin.0 + screen_width.saturating_sub(margin as u32) as i32,
            origin.1 + screen_height.saturating_sub(margin as u32) as i32,
        ),
    ];
    points
        .into_iter()
        .find(|(x, y)| {
            *x < left - margin || *x > right + margin || *y < top - margin || *y > bottom + margin
        })
        .unwrap_or((
            origin.0 + screen_width as i32 / 2,
            origin.1 + screen_height as i32 / 2,
        ))
}

fn move_smooth(
    enigo: &mut Enigo,
    start: (i32, i32),
    end: (i32, i32),
    attempt: u8,
) -> AppResult<()> {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let distance = (((dx * dx + dy * dy) as f64).sqrt()).max(1.0);
    let steps = ((distance / 8.0).ceil() as usize).clamp(8, 80);
    let perpendicular = (-dy as f64 / distance, dx as f64 / distance);
    let bend = ((attempt % 5) as f64 - 2.0) * distance.min(120.0) / 14.0;
    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        let eased = (1.0 - (std::f64::consts::PI * t).cos()) / 2.0;
        let curve = (std::f64::consts::PI * t).sin() * bend;
        let phase = attempt as f64 * 1.37 + distance * 0.013;
        let micro_deviation = (std::f64::consts::PI * t).sin()
            * (std::f64::consts::PI * 3.0 * t + phase).sin()
            * distance.min(18.0)
            / 7.0;
        let x = start.0 as f64 + dx as f64 * eased + perpendicular.0 * (curve + micro_deviation);
        let y = start.1 as f64 + dy as f64 * eased + perpendicular.1 * (curve + micro_deviation);
        enigo
            .move_mouse(x.round() as i32, y.round() as i32, Coordinate::Abs)
            .map_err(|e| AppError::Input(e.to_string()))?;
        let speed_variation = ((std::f64::consts::PI * 2.7 * t + phase).sin() + 1.0) * 2.5;
        let base_pause = if cfg!(target_os = "windows") {
            10.0
        } else {
            3.0
        };
        let pause = (base_pause + (1.0 - (2.0 * t - 1.0).abs()) * 8.0 + speed_variation) as u64;
        thread::sleep(Duration::from_millis(pause));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_stay_inside_zone() {
        let zone = Zone {
            name: "send".into(),
            kind: "action".into(),
            purpose: "submit_prompt".into(),
            x: 10,
            y: 20,
            width: 100,
            height: 40,
            screen_index: 0,
            sample_busy: None,
            sample_ready: None,
            sample_empty: None,
        };
        for attempt in 0..8 {
            let (x, y) = sample_point(&zone, attempt, (0, 0));
            assert!((10..=110).contains(&x) && (20..=60).contains(&y));
        }
    }

    #[test]
    fn safe_point_is_outside_observation_zone() {
        let zone = Zone {
            name: "generation_state".into(),
            kind: "observation".into(),
            purpose: "detect_generation_finished".into(),
            x: 900,
            y: 700,
            width: 80,
            height: 80,
            screen_index: 0,
            sample_busy: None,
            sample_ready: None,
            sample_empty: None,
        };
        let point = safe_point_outside(&zone, (0, 0), 1200, 800);
        assert!(
            point.0 < zone.x - 24
                || point.0 > zone.x + zone.width as i32 + 24
                || point.1 < zone.y - 24
                || point.1 > zone.y + zone.height as i32 + 24
        );
    }
}
