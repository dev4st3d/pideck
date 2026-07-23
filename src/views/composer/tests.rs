use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    Context, Entity, Focusable, IntoElement, MouseButton, Render, Subscription, TestAppContext,
    Window, div, prelude::*,
};

use super::*;
use crate::actions::{ComposerBackspace, ComposerPaste, ComposerUndo, composer_key_bindings};

struct ComposerHarness {
    composer: Entity<Composer>,
    events: Rc<RefCell<Vec<ComposerEvent>>>,
    _subscription: Subscription,
}

impl ComposerHarness {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(Composer::new);
        composer.update(cx, |composer, cx| {
            composer.set_availability(ComposerAvailability::Idle, cx)
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = Rc::clone(&events);
        let subscription = cx.subscribe(&composer, move |_, _, event, _| {
            received.borrow_mut().push(event.clone());
        });
        window.focus(&composer.read(cx).focus_handle(cx));
        window.activate_window();
        Self {
            composer,
            events,
            _subscription: subscription,
        }
    }
}

impl Render for ComposerHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.composer.clone())
    }
}

#[gpui::test]
fn composer_actions_route_send_newline_follow_up_and_abort(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    cx.simulate_input("hello");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("world");
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "hello\nworld"
    );

    cx.simulate_keystrokes("enter");
    let events = harness.read_with(cx, |harness, _| harness.events.borrow().clone());
    assert_eq!(
        events.as_slice(),
        &[ComposerEvent::Accept {
            text: "hello\nworld".to_owned(),
            images: Vec::new(),
        }]
    );

    composer.update(cx, |composer, cx| {
        composer.set_feedback(ComposerFeedback::Pending(SubmissionKind::Prompt), cx)
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().len()),
        1
    );

    composer.update(cx, |composer, cx| {
        composer.set_feedback(ComposerFeedback::Ready, cx);
        composer.set_availability(ComposerAvailability::Running, cx)
    });
    cx.simulate_keystrokes("alt-enter escape");
    let events = harness.read_with(cx, |harness, _| harness.events.borrow().clone());
    assert_eq!(
        events.as_slice(),
        &[
            ComposerEvent::Accept {
                text: "hello\nworld".to_owned(),
                images: Vec::new(),
            },
            ComposerEvent::FollowUp {
                text: "hello\nworld".to_owned(),
                images: Vec::new(),
            },
            ComposerEvent::Abort,
        ]
    );
}

#[gpui::test]
fn escape_routes_bash_abort_separately_from_agent_abort(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());
    composer.update(cx, |composer, cx| {
        composer.set_availability(ComposerAvailability::BashRunning, cx)
    });

    cx.simulate_keystrokes("escape");
    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().clone()),
        vec![ComposerEvent::AbortBash]
    );
}

#[gpui::test]
fn command_completion_owns_arrows_enter_and_escape_without_mutating_draft(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());
    cx.simulate_input("/comp");
    composer.update(cx, |composer, cx| {
        composer.set_command_completion_active(true, cx)
    });

    cx.simulate_keystrokes("down up enter escape");
    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().clone()),
        vec![
            ComposerEvent::CommandNext,
            ComposerEvent::CommandPrevious,
            ComposerEvent::CommandAccept,
            ComposerEvent::CommandDismiss,
        ]
    );
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "/comp"
    );
}

#[gpui::test]
fn composer_input_is_grapheme_safe_and_supports_clipboard_undo(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    cx.update(|window, app| {
        composer.update(app, |composer, cx| {
            composer.replace_text_in_range(None, "👩‍💻e\u{301}", window, cx)
        });
    });
    cx.dispatch_action(ComposerBackspace);
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "👩‍💻"
    );
    cx.dispatch_action(ComposerBackspace);
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        ""
    );
    cx.dispatch_action(ComposerUndo);
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "👩‍💻"
    );

    cx.write_to_clipboard(ClipboardItem::new_string("one\r\ntwo".to_owned()));
    cx.dispatch_action(ComposerPaste);
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "👩‍💻one\ntwo"
    );
}

#[gpui::test]
fn composer_pastes_clipboard_images_and_submits_them_without_text(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());
    let bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.clone());

    cx.write_to_clipboard(ClipboardItem::new_image(&image));
    cx.dispatch_action(ComposerPaste);

    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        ""
    );
    let attached = composer.read_with(cx, |composer, _| composer.images().to_vec());
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].mime_type, "image/png");
    assert_eq!(attached[0].data, STANDARD.encode(bytes));

    cx.simulate_keystrokes("enter");
    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().clone()),
        vec![ComposerEvent::Accept {
            text: String::new(),
            images: attached,
        }]
    );
    assert!(composer.update(cx, |composer, cx| {
        composer.clear_accepted("", SubmissionKind::Prompt, cx)
    }));
    assert!(!composer.read_with(cx, |composer, _| composer.has_images()));
}

#[gpui::test]
fn composer_ime_uses_utf16_ranges_and_retains_disabled_draft(cx: &mut TestAppContext) {
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    cx.update(|window, app| {
        composer.update(app, |composer, cx| {
            composer.replace_text_in_range(None, "A😀B", window, cx);
            composer.replace_and_mark_text_in_range(Some(1..3), "日本語", Some(2..3), window, cx);
            assert_eq!(composer.marked_text_range(window, cx), Some(1..4));
            let selection = composer
                .selected_text_range(false, window, cx)
                .expect("IME selection");
            assert_eq!(selection.range, 3..4);
            assert!(!selection.reversed);
        });
    });
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "A日本語B"
    );

    composer.update(cx, |composer, cx| {
        composer.set_availability(ComposerAvailability::Unavailable, cx)
    });
    cx.update(|window, app| {
        composer.update(app, |composer, cx| {
            assert!(composer.selected_text_range(false, window, cx).is_none());
            composer.replace_text_in_range(None, "discarded", window, cx);
        });
    });
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "A日本語B"
    );
}

#[gpui::test]
fn composer_mouse_selection_scroll_and_delivery_feedback_preserve_drafts(cx: &mut TestAppContext) {
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    cx.simulate_input("alpha beta");
    let (start, end) = composer.read_with(cx, |composer, _| {
        let layout = composer.last_layout.as_ref().expect("painted layout");
        (
            layout.position_for_offset(0) + gpui::point(px(1.0), px(4.0)),
            layout.position_for_offset(5) + gpui::point(px(1.0), px(4.0)),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(end, MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, gpui::Modifiers::none());
    assert_eq!(
        composer.read_with(cx, |composer, _| composer
            .buffer
            .selected_text()
            .map(ToOwned::to_owned)),
        Some("alpha".to_owned())
    );

    cx.update(|window, app| {
        composer.update(app, |composer, cx| {
            composer.replace_text_in_range(
                Some(0..10),
                &(0..20)
                    .map(|line| format!("line {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                window,
                cx,
            )
        });
    });
    cx.run_until_parked();
    assert!(composer.read_with(cx, |composer, _| {
        composer
            .last_layout
            .as_ref()
            .is_some_and(|layout| layout.content_height > layout.bounds.size.height)
            && composer.scroll_y > Pixels::ZERO
    }));

    let draft = composer.read_with(cx, |composer, _| composer.draft().to_owned());
    composer.update(cx, |composer, cx| {
        composer.set_feedback(
            ComposerFeedback::Rejected("Pi rejected the prompt. Draft kept.".to_owned()),
            cx,
        )
    });
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        draft
    );
    assert!(!composer.update(cx, |composer, cx| {
        composer.clear_accepted("different revision", SubmissionKind::Prompt, cx)
    }));
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        draft
    );
    assert!(composer.update(cx, |composer, cx| {
        composer.clear_accepted(&draft, SubmissionKind::Prompt, cx)
    }));
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        ""
    );
}
