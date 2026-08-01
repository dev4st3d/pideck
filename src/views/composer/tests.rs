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
            files: Vec::new(),
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
                files: Vec::new(),
            },
            ComposerEvent::FollowUp {
                text: "hello\nworld".to_owned(),
                images: Vec::new(),
                files: Vec::new(),
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
fn composer_ctrl_backspace_deletes_the_previous_word(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    cx.simulate_input("alpha café   ");
    cx.simulate_keystrokes("ctrl-backspace");
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.draft().to_owned()),
        "alpha "
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
            files: Vec::new(),
        }]
    );
    assert!(composer.update(cx, |composer, cx| {
        composer.clear_accepted("", SubmissionKind::Prompt, cx)
    }));
    assert!(!composer.read_with(cx, |composer, _| composer.has_images()));
}

#[gpui::test]
fn composer_submits_and_restores_text_file_attachments(cx: &mut TestAppContext) {
    use crate::attachments::{FileDelivery, PromptFileMetadata};

    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());
    let file = PromptFile {
        metadata: PromptFileMetadata {
            name: "notes.md".to_owned(),
            path: "C:/work/notes.md".to_owned(),
            size: 7,
            delivery: FileDelivery::Snapshot,
        },
        content: Some(Arc::from("# Notes")),
    };
    composer.update(cx, |composer, cx| {
        composer.add_loaded_attachments(
            LoadedAttachmentBatch {
                attachments: vec![LoadedAttachment::File(file.clone())],
                issues: Vec::new(),
            },
            cx,
        )
    });

    assert!(composer.read_with(cx, |composer, _| composer.has_attachments()));
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.files().to_vec()),
        vec![file.clone()]
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().clone()),
        vec![ComposerEvent::Accept {
            text: String::new(),
            images: Vec::new(),
            files: vec![file.clone()],
        }]
    );

    composer.update(cx, |composer, cx| {
        composer.restore_draft("", Vec::new(), vec![file], cx);
    });
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.file_attach_token(0)),
        0
    );
    composer.update(cx, |composer, cx| composer.remove_file(0, cx));
    assert!(!composer.read_with(cx, |composer, _| composer.has_attachments()));
}

#[gpui::test]
fn composer_emits_preview_for_an_attached_image_only(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());
    let image = gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
    );

    cx.write_to_clipboard(ClipboardItem::new_image(&image));
    cx.dispatch_action(ComposerPaste);
    composer.update(cx, |composer, cx| {
        composer.preview_image(0, cx);
        composer.preview_image(1, cx);
    });

    assert_eq!(
        harness.read_with(cx, |harness, _| harness.events.borrow().clone()),
        vec![ComposerEvent::PreviewImage(0)]
    );
}

#[gpui::test]
fn composer_builds_square_thumbnails_for_decodable_images(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys(composer_key_bindings()));
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    let source = image::RgbaImage::from_pixel(24, 12, image::Rgba([220, 40, 40, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(source)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode test png");
    let bytes = encoded.into_inner();
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes);

    cx.write_to_clipboard(ClipboardItem::new_image(&image));
    cx.dispatch_action(ComposerPaste);

    assert!(composer.read_with(cx, |composer, _| composer.thumbnail(0).is_some()));
    assert!(composer.read_with(cx, |composer, _| composer.thumbnail(1).is_none()));
    // Fresh paste should get a non-zero motion token for the pop-in animation.
    assert_ne!(
        composer.read_with(cx, |composer, _| composer.attach_token(0)),
        0
    );
    assert_ne!(
        composer.read_with(cx, |composer, _| composer.strip_motion_key()),
        0
    );

    let restored = composer.read_with(cx, |composer, _| composer.images().to_vec());
    composer.update(cx, |composer, cx| {
        composer.restore_draft("", restored, Vec::new(), cx);
    });
    // Restored drafts settle immediately (token 0 skips the pop).
    assert_eq!(
        composer.read_with(cx, |composer, _| composer.attach_token(0)),
        0
    );

    composer.update(cx, |composer, cx| composer.remove_image(0, cx));
    assert!(!composer.read_with(cx, |composer, _| composer.has_images()));
    assert!(composer.read_with(cx, |composer, _| composer.thumbnail(0).is_none()));
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

#[gpui::test]
fn height_hold_keeps_input_expanded_while_a_prompt_sheet_owns_focus(cx: &mut TestAppContext) {
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    composer.update(cx, |composer, cx| composer.set_input_expanded(true, cx));
    // Let the expand motion finish so later steps can observe new motions only.
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    assert!(composer.read_with(cx, |composer, _| composer.input_height_motion().is_none()));

    // A prompt sheet opens: the height pins before focus leaves for the sheet.
    cx.update(|window, app| {
        composer.update(app, |composer, cx| {
            composer.set_height_hold(true, window, cx)
        });
    });
    // The blur that follows clicking the tray must not start a collapse.
    composer.update(cx, |composer, cx| composer.set_input_expanded(false, cx));
    assert!(composer.read_with(cx, |composer, _| composer.input_expanded));
    assert!(
        composer.read_with(cx, |composer, _| composer.input_height_motion().is_none()),
        "blur under height hold must not start a collapse motion"
    );

    // Release without focus: one direct motion back to the collapsed shell.
    cx.update(|window, app| {
        window.blur();
        composer.update(app, |composer, cx| {
            composer.set_height_hold(false, window, cx)
        });
    });
    assert!(!composer.read_with(cx, |composer, _| composer.input_expanded));
    let motion = composer.read_with(cx, |composer, _| composer.input_height_motion());
    let collapsed = 20.0 + 8.0 * 2.0; // one line + desk padding
    let motion = motion.expect("release should animate the collapse");
    assert!((motion.from - 56.0).abs() < 0.5, "from={:?}", motion.from);
    assert!((motion.to - collapsed).abs() < 0.5, "to={:?}", motion.to);
}

#[gpui::test]
fn minimize_enlarged_input_animates_to_normal_not_collapsed(cx: &mut TestAppContext) {
    let (harness, cx) = cx.add_window_view(ComposerHarness::new);
    let composer = harness.read_with(cx, |harness, _| harness.composer.clone());

    // Focused multi-line shell, then pin enlarge.
    composer.update(cx, |composer, cx| {
        composer.set_input_expanded(true, cx);
        composer.toggle_input_enlarged(cx);
    });
    let enlarged_h = composer.read_with(cx, |composer, _| composer.input_target_height());
    assert!(composer.read_with(cx, |composer, _| composer.input_enlarged()));
    assert!((enlarged_h - 152.0).abs() < 0.5);

    // Clicking the chrome control blurs the field before the toggle runs.
    composer.update(cx, |composer, cx| {
        composer.set_input_expanded(false, cx);
        composer.toggle_input_enlarged(cx);
    });

    let (enlarged, target, motion) = composer.read_with(cx, |composer, _| {
        (
            composer.input_enlarged(),
            composer.input_target_height(),
            composer.input_height_motion(),
        )
    });
    assert!(!enlarged);
    // Must settle on multi-line (56), not idle single-line (~36).
    assert!(
        (target - 56.0).abs() < 0.5,
        "minimize target should be normal multi-line, got {target}"
    );
    let motion = motion.expect("minimize should animate height");
    assert!(
        (motion.from - 152.0).abs() < 0.5,
        "minimize from={:?}",
        motion.from
    );
    assert!(
        (motion.to - 56.0).abs() < 0.5,
        "minimize to={:?}",
        motion.to
    );

    // Root re-focuses the field after toggle; that must not replace the motion.
    composer.update(cx, |composer, cx| {
        composer.set_input_expanded(true, cx);
    });
    let motion_after_focus = composer.read_with(cx, |composer, _| composer.input_height_motion());
    let motion_after_focus = motion_after_focus.expect("re-focus should keep minimize motion");
    assert_eq!(motion_after_focus.generation, motion.generation);
    assert!((motion_after_focus.to - 56.0).abs() < 0.5);
}
