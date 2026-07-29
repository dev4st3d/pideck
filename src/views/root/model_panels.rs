use super::shared::{popup_sheet, popup_sheet_header};
use super::*;

pub(super) struct ModelSettingsPanelParams<'a> {
    pub(super) projection: &'a ModelRuntimeProjection,
    pub(super) resources: &'a ResourceCenterProjection,
    pub(super) tab: ModelSettingsTab,
    pub(super) resource_scope_filter: ResourceScopeFilter,
    pub(super) resource_state_filter: ResourceStateFilter,
    pub(super) search: &'a Entity<Composer>,
    pub(super) font_search: &'a Entity<Composer>,
    pub(super) font_catalog: &'a FontCatalog,
    pub(super) font_role: FontRole,
    pub(super) font_feedback: Option<&'a str>,
    pub(super) pi_scroll: &'a ScrollHandle,
}

pub(super) fn model_settings_panel(
    params: ModelSettingsPanelParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ModelSettingsPanelParams {
        projection,
        resources,
        tab,
        resource_scope_filter,
        resource_state_filter,
        search,
        font_search,
        font_catalog,
        font_role,
        font_feedback,
        pi_scroll,
    } = params;
    let refreshing = if tab == ModelSettingsTab::Resources {
        matches!(resources.phase, ResourcePhase::Refreshing)
    } else {
        matches!(projection.phase, CatalogPhase::Refreshing)
    };
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .child(
            div()
                .px(px(18.0))
                .pt(px(14.0))
                .pb(px(12.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_UI))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(if tab == ModelSettingsTab::Resources {
                                            "Resource Center"
                                        } else if tab == ModelSettingsTab::Typography {
                                            "Typography"
                                        } else if tab == ModelSettingsTab::Pi {
                                            "Pi settings"
                                        } else {
                                            "Model settings"
                                        }),
                                )
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .text_color(theme::ash())
                                        .child(
                                            if tab == ModelSettingsTab::Resources {
                                                "Audited Pi resources, provenance, trust, load state, and active tools."
                                            } else if tab == ModelSettingsTab::Typography {
                                                "Choose any installed system font for the app's three text roles."
                                            } else if tab == ModelSettingsTab::Pi {
                                                "Typed controls backed by Pi's SettingsManager and effective global values."
                                            } else {
                                                "Providers, defaults, cycle order, and usage. Session model and thinking live in the prompt box."
                                            },
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .flex_shrink_0()
                                .when(tab != ModelSettingsTab::Typography, |actions| {
                                    actions.child(controls::quiet_button(
                                        "refresh-model-catalog",
                                        if refreshing {
                                            "Refreshing…"
                                        } else if tab == ModelSettingsTab::Resources {
                                            "Reload"
                                        } else {
                                            "Refresh"
                                        },
                                        !refreshing,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            if tab == ModelSettingsTab::Resources {
                                                view.reload_resources(cx);
                                            } else {
                                                view.refresh_models(cx);
                                            }
                                        })),
                                    ))
                                })
                                .child(controls::quiet_button(
                                    "close-model-settings",
                                    "Done",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.close_model_panel(window, cx)
                                    })),
                                )),
                        ),
                )
                .child(
                    controls::tab_track().children(
                        [
                            (ModelSettingsTab::Providers, "Providers"),
                            (ModelSettingsTab::Models, "Models"),
                            (ModelSettingsTab::Thinking, "Thinking"),
                            (ModelSettingsTab::Pi, "Pi"),
                            (ModelSettingsTab::Usage, "Usage"),
                            (ModelSettingsTab::Typography, "Type"),
                            (ModelSettingsTab::Resources, "Resources"),
                        ]
                        .into_iter()
                        .map(|(target, label)| {
                            controls::tab_button(
                                gpui::SharedString::from(format!("model-tab-{label}")),
                                label,
                                tab == target,
                                Box::new(cx.listener(move |view, _, window, cx| {
                                    view.set_model_settings_tab(target, window, cx)
                                })),
                            )
                        }),
                    ),
                ),
        )
        .child(match tab {
            ModelSettingsTab::Providers => providers_settings(projection, cx).into_any_element(),
            ModelSettingsTab::Models => models_settings(projection, search, cx).into_any_element(),
            ModelSettingsTab::Thinking => thinking_settings(projection, cx).into_any_element(),
            ModelSettingsTab::Pi => pi_settings(projection, pi_scroll, cx),
            ModelSettingsTab::Usage => usage_settings(projection).into_any_element(),
            ModelSettingsTab::Typography => typography_settings(
                font_catalog,
                font_role,
                font_search,
                cx,
            )
            .into_any_element(),
            ModelSettingsTab::Resources => resource_center_settings(
                resources,
                resource_scope_filter,
                resource_state_filter,
                cx,
            )
            .into_any_element(),
        })
        .when(
            tab != ModelSettingsTab::Resources && tab != ModelSettingsTab::Typography,
            |panel| {
                panel
                .when_some(catalog_phase_note(&projection.phase), |panel, note| {
                    panel.child(controls::panel_footer_status(note))
                })
                .when_some(projection.feedback.clone(), |panel, feedback| {
                    panel.child(controls::panel_footer_status(feedback))
                })
            },
        )
        .when(tab == ModelSettingsTab::Typography, |panel| {
            panel.when_some(font_feedback.map(str::to_owned), |panel, feedback| {
                panel.child(controls::panel_footer_status(feedback))
            })
        })
        .when(tab == ModelSettingsTab::Resources, |panel| {
            panel.when_some(resources.feedback.clone(), |panel, feedback| {
                panel.child(controls::panel_footer_status(feedback))
            })
        })
}

fn typography_settings(
    catalog: &FontCatalog,
    active_role: FontRole,
    search: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft().trim().to_lowercase();
    let families = catalog
        .families
        .iter()
        .filter(|family| query.is_empty() || family.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    let count = families.len();
    let selected_family = catalog.preferences.family(active_role).to_owned();

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(18.0))
                .py(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(div().flex().flex_row().gap(px(8.0)).children(
                    FontRole::ALL.into_iter().map(|role| {
                        let selected = role == active_role;
                        let family = catalog.preferences.family(role).to_owned();
                        div()
                            .id(gpui::SharedString::from(format!(
                                "font-role-{}",
                                role.label().to_lowercase()
                            )))
                            .flex_1()
                            .min_w_0()
                            .p(px(10.0))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(if selected {
                                theme::panel_lift()
                            } else {
                                theme::panel()
                            })
                            .border_1()
                            .border_color(if selected {
                                theme::focus()
                            } else {
                                theme::edge_soft()
                            })
                            .tab_index(0)
                            .cursor_pointer()
                            .hover(|card| card.bg(theme::panel_hover()))
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.set_font_role(role, cx)),
                            )
                            .on_key_down(cx.listener(
                                move |view, event: &gpui::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        cx.stop_propagation();
                                        view.set_font_role(role, cx);
                                    }
                                },
                            ))
                            .child(
                                div()
                                    .font_family(theme::main())
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if selected {
                                        theme::bone()
                                    } else {
                                        theme::bone_dim()
                                    })
                                    .child(role.label()),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .font_family(family.clone())
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .text_color(theme::ash())
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(family),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(role.description()),
                            )
                    }),
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(div().flex_1().min_w_0().child(search.clone()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(format!("{count} fonts")),
                        ),
                ),
        )
        .child(
            div()
                .id("system-font-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .when(families.is_empty(), |list| {
                    list.child(
                        div()
                            .px(px(18.0))
                            .py(px(16.0))
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .text_color(theme::smoke())
                            .child("No installed fonts match this search."),
                    )
                })
                .children(families.into_iter().map(|family| {
                    let selected = family == selected_family;
                    let keyboard_family = family.clone();
                    let click_family = family.clone();
                    div()
                        .id(gpui::SharedString::from(format!(
                            "font-{}-{}",
                            active_role.label().to_lowercase(),
                            family
                        )))
                        .min_h(px(46.0))
                        .px(px(18.0))
                        .py(px(8.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .border_b_1()
                        .border_color(theme::edge_soft())
                        .bg(if selected {
                            theme::panel_lift()
                        } else {
                            theme::canvas()
                        })
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|row| row.bg(theme::panel_hover()))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.select_font(active_role, click_family.clone(), cx)
                        }))
                        .on_key_down(cx.listener(move |view, event: &gpui::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                view.select_font(active_role, keyboard_family.clone(), cx);
                            }
                        }))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .font_family(family.clone())
                                .text_size(theme::text_size(theme::T_BODY))
                                .text_color(theme::bone_dim())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(family),
                        )
                        .when(selected, |row| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .font_family(theme::main())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::focus())
                                    .child("Selected"),
                            )
                        })
                })),
        )
}

pub(super) fn model_switcher_sheet(
    projection: &ModelRuntimeProjection,
    provider_filter: Option<&str>,
    search: &Entity<Composer>,
    model_scroll: &ScrollHandle,
    provider_scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let mut providers = model_provider_choices(projection);
    if let Some(active_provider) = projection
        .active_model
        .as_ref()
        .map(|identity| identity.provider.as_str())
        && let Some(index) = providers
            .iter()
            .position(|provider| provider.id == active_provider)
        && index > 0
    {
        let active = providers.remove(index);
        providers.insert(0, active);
    }
    let selected_provider =
        provider_filter.filter(|provider| providers.iter().any(|choice| choice.id == *provider));
    let selected_provider_name = selected_provider
        .and_then(|provider| providers.iter().find(|choice| choice.id == provider))
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| "All models".to_owned());
    let query = search.read(cx).draft().to_owned();
    let models = model_choices(projection, selected_provider, &query);
    let active = projection.active_model.clone();
    let can_change = projection.model_change_policy == ModelChangePolicy::Allowed;
    let total_models = providers.iter().map(|provider| provider.model_count).sum();
    let empty_message = match projection.phase {
        CatalogPhase::Loading => "Loading models…".to_owned(),
        _ if query.trim().is_empty() => selected_provider
            .and_then(|provider| providers.iter().find(|choice| choice.id == provider))
            .map(|provider| format!("No available {} models.", provider.name))
            .unwrap_or_else(|| "No available models.".to_owned()),
        _ => "No models match this search.".to_owned(),
    };
    let policy_note = match projection.model_change_policy {
        ModelChangePolicy::Allowed => None,
        ModelChangePolicy::WaitUntilIdle => {
            Some("Wait for the current response to finish before switching models.")
        }
        ModelChangePolicy::RuntimeUnavailable => Some("Reconnect Pi before switching models."),
    };

    popup_sheet()
        .id("model-switcher-sheet")
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                view.close_model_panel(window, cx);
            }
        }))
        // Definite flex height keeps both independent scroll panes inside the sheet.
        .h(px(410.0))
        .max_w(px(720.0))
        .child(
            div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_row()
                .child(
                    div()
                        .w(px(62.0))
                        .h_full()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .bg(theme::floor())
                        .border_r_1()
                        .border_color(theme::edge_soft())
                        .child(model_provider_button(
                            None,
                            selected_provider.is_none(),
                            total_models,
                            cx,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .relative()
                                .child(controls::scroll_wheel_capture(provider_scroll))
                                .child(
                                    div()
                                        .id("model-provider-scroll")
                                        .size_full()
                                        .pt(px(4.0))
                                        .overflow_y_scroll()
                                        .track_scroll(provider_scroll)
                                        .scrollbar_width(px(4.0))
                                        .children(providers.iter().map(|provider| {
                                            model_provider_button(
                                                Some(provider),
                                                selected_provider == Some(provider.id.as_str()),
                                                provider.model_count,
                                                cx,
                                            )
                                        })),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .bg(theme::panel())
                        .child(
                            div()
                                .h(px(48.0))
                                .px(px(12.0))
                                .flex_shrink_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .border_b_1()
                                .border_color(theme::edge_soft())
                                .child(
                                    div()
                                        .w(px(116.0))
                                        .flex_shrink_0()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_LABEL))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::bone_dim())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(selected_provider_name),
                                )
                                .child(div().flex_1().min_w_0().child(search.clone()))
                                .child(model_switcher_close_button(cx)),
                        )
                        .when_some(policy_note, |list, note| {
                            list.child(
                                div()
                                    .px(px(12.0))
                                    .py(px(7.0))
                                    .flex_shrink_0()
                                    .bg(theme::canvas())
                                    .border_b_1()
                                    .border_color(theme::edge_soft())
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::ash())
                                    .child(note),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .relative()
                                .child(controls::scroll_wheel_capture(model_scroll))
                                .child(
                                    div()
                                        .id("model-switcher-scroll")
                                        .size_full()
                                        .py(px(6.0))
                                        .bg(theme::panel())
                                        .overflow_y_scroll()
                                        .track_scroll(model_scroll)
                                        .scrollbar_width(px(6.0))
                                        .when(models.is_empty(), |list| {
                                            list.child(
                                                div()
                                                    .px(px(16.0))
                                                    .py(px(18.0))
                                                    .font_family(theme::sans())
                                                    .text_size(theme::text_size(theme::T_UI_SM))
                                                    .text_color(theme::smoke())
                                                    .child(empty_message.clone()),
                                            )
                                        })
                                        .children(models.into_iter().map(|model| {
                                            let click_identity = model.identity.clone();
                                            let keyboard_identity = model.identity.clone();
                                            let selected = active.as_ref() == Some(&model.identity);
                                            let context = format!(
                                                "{} context",
                                                compact_count(model.context_window)
                                            );
                                            div()
                                                .id(gpui::SharedString::from(format!(
                                                    "switch-model-{}-{}",
                                                    model.identity.provider, model.identity.id
                                                )))
                                                .min_h(px(58.0))
                                                .mx(px(8.0))
                                                .my(px(2.0))
                                                .px(px(12.0))
                                                .py(px(8.0))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(12.0))
                                                .rounded(px(theme::RADIUS))
                                                .border_1()
                                                .border_color(if selected {
                                                    theme::edge_hard()
                                                } else {
                                                    theme::panel()
                                                })
                                                .bg(if selected {
                                                    theme::panel_lift()
                                                } else {
                                                    theme::panel()
                                                })
                                                .when(can_change && !selected, |row| {
                                                    row.tab_index(0)
                                                        .cursor_pointer()
                                                        .hover(|row| row.bg(theme::panel_lift()))
                                                        .active(|row| row.bg(theme::panel_hover()))
                                                        .focus(|row| {
                                                            row.bg(theme::panel_lift())
                                                                .border_color(theme::focus())
                                                        })
                                                        .on_click(cx.listener(
                                                            move |view, _, window, cx| {
                                                                view.select_model(
                                                                    click_identity.clone(),
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        ))
                                                        .on_key_down(cx.listener(
                                                            move |view,
                                                                  event: &gpui::KeyDownEvent,
                                                                  window,
                                                                  cx| {
                                                                if matches!(
                                                                    event.keystroke.key.as_str(),
                                                                    "enter" | "space"
                                                                ) {
                                                                    cx.stop_propagation();
                                                                    view.select_model(
                                                                        keyboard_identity.clone(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                }
                                                            },
                                                        ))
                                                })
                                                .child(
                                                    div()
                                                        .min_w_0()
                                                        .flex_1()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(3.0))
                                                        .child(
                                                            div()
                                                                .font_family(theme::sans())
                                                                .text_size(theme::text_size(
                                                                    theme::T_UI,
                                                                ))
                                                                .font_weight(if selected {
                                                                    FontWeight::BOLD
                                                                } else {
                                                                    FontWeight::SEMIBOLD
                                                                })
                                                                .text_color(if !can_change {
                                                                    theme::smoke()
                                                                } else if selected {
                                                                    theme::bone()
                                                                } else {
                                                                    theme::bone_dim()
                                                                })
                                                                .overflow_hidden()
                                                                .text_ellipsis()
                                                                .whitespace_nowrap()
                                                                .child(model.name),
                                                        )
                                                        .child(
                                                            div()
                                                                .font_family(theme::sans())
                                                                .text_size(theme::text_size(
                                                                    theme::T_LABEL,
                                                                ))
                                                                .text_color(theme::smoke())
                                                                .overflow_hidden()
                                                                .text_ellipsis()
                                                                .whitespace_nowrap()
                                                                .child(model.provider_name),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .flex_col()
                                                        .items_end()
                                                        .gap(px(3.0))
                                                        .child(
                                                            div()
                                                                .font_family(theme::mono())
                                                                .text_size(theme::text_size(
                                                                    theme::T_TINY,
                                                                ))
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .text_color(theme::ash())
                                                                .child(context),
                                                        )
                                                        .when(selected, |meta| {
                                                            meta.child(
                                                                div()
                                                                    .font_family(theme::sans())
                                                                    .text_size(theme::text_size(
                                                                        theme::T_TINY,
                                                                    ))
                                                                    .font_weight(
                                                                        FontWeight::SEMIBOLD,
                                                                    )
                                                                    .text_color(theme::data())
                                                                    .child("Active"),
                                                            )
                                                        }),
                                                )
                                        })),
                                ),
                        )
                        .when_some(projection.feedback.clone(), |list, feedback| {
                            list.child(
                                div()
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .flex_shrink_0()
                                    .bg(theme::panel())
                                    .border_t_1()
                                    .border_color(theme::edge_soft())
                                    .font_family(theme::mono())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(feedback),
                            )
                        }),
                ),
        )
}

fn model_switcher_close_button(cx: &mut Context<RootView>) -> gpui::AnyElement {
    div()
        .id("close-model-switcher")
        .h(px(26.0))
        .px(px(8.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .tab_index(0)
        .cursor_pointer()
        .font_family(theme::main())
        .text_size(theme::text_size(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ash())
        .hover(|button| button.bg(theme::canvas()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .focus(|button| button.bg(theme::canvas()).text_color(theme::focus()))
        .on_click(cx.listener(|view, _, window, cx| view.close_model_panel(window, cx)))
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                view.close_model_panel(window, cx);
            }
        }))
        .child("Close")
        .into_any_element()
}

fn model_provider_button(
    provider: Option<&ModelProviderChoice>,
    selected: bool,
    model_count: usize,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let provider_id = provider.map(|provider| provider.id.clone());
    let click_provider = provider_id.clone();
    let keyboard_provider = provider_id.clone();
    let name = provider
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| "All providers".to_owned());
    let mark = provider
        .map(|provider| provider_mark(&provider.id, &provider.name))
        .unwrap_or_else(|| "ALL".to_owned());
    let detail = format!(
        "{model_count} available model{}",
        if model_count == 1 { "" } else { "s" }
    );
    let tooltip_name = name.clone();
    let tooltip_detail = detail.clone();

    div()
        .id(gpui::SharedString::from(match provider_id {
            Some(provider) => format!("model-provider-{provider}"),
            None => "model-provider-all".to_owned(),
        }))
        .relative()
        .h(px(48.0))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .tab_index(0)
        .cursor_pointer()
        .bg(if selected {
            theme::panel_lift()
        } else {
            theme::floor()
        })
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .hover(move |button| {
            button
                .bg(if selected {
                    theme::panel_hover()
                } else {
                    theme::panel()
                })
                .text_color(if selected {
                    theme::bone()
                } else {
                    theme::bone_dim()
                })
        })
        .active(|button| button.bg(theme::panel_hover()))
        .focus(|button| button.bg(theme::panel()).text_color(theme::focus()))
        .on_click(cx.listener(move |view, _, _, cx| {
            view.set_model_provider_filter(click_provider.clone(), cx)
        }))
        .on_key_down(cx.listener(move |view, event: &gpui::KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                view.set_model_provider_filter(keyboard_provider.clone(), cx);
            }
        }))
        .tooltip(move |_, cx| {
            cx.new(|_| ModelProviderTooltip {
                name: tooltip_name.clone(),
                detail: tooltip_detail.clone(),
            })
            .into()
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(if mark.chars().count() > 2 {
                    9.5
                } else {
                    11.5
                }))
                .font_weight(FontWeight::BOLD)
                .child(mark),
        )
        .when(selected, |button| {
            button.child(
                div()
                    .absolute()
                    .right_0()
                    .top(px(11.0))
                    .bottom(px(11.0))
                    .w(px(2.0))
                    .rounded(px(1.0))
                    .bg(theme::focus()),
            )
        })
        .into_any_element()
}

struct ModelProviderTooltip {
    name: String,
    detail: String,
}

impl Render for ModelProviderTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(9.0))
            .py(px(7.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_LABEL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::bone())
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_TINY))
                    .text_color(theme::ash())
                    .child(self.detail.clone()),
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelProviderChoice {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) model_count: usize,
}

pub(super) fn model_provider_choices(
    projection: &ModelRuntimeProjection,
) -> Vec<ModelProviderChoice> {
    if let Some(catalog) = projection.catalog.as_ref() {
        let mut choices = Vec::new();
        let mut known = HashSet::new();
        for provider in &catalog.providers {
            let model_count = catalog
                .models
                .iter()
                .filter(|model| model.available && model.identity.provider == provider.id)
                .count();
            if model_count > 0 && known.insert(provider.id.clone()) {
                choices.push(ModelProviderChoice {
                    id: provider.id.clone(),
                    name: provider.name.clone(),
                    model_count,
                });
            }
        }
        for model in catalog.models.iter().filter(|model| model.available) {
            if known.insert(model.identity.provider.clone()) {
                choices.push(ModelProviderChoice {
                    id: model.identity.provider.clone(),
                    name: provider_display_name(&model.identity.provider),
                    model_count: catalog
                        .models
                        .iter()
                        .filter(|candidate| {
                            candidate.available
                                && candidate.identity.provider == model.identity.provider
                        })
                        .count(),
                });
            }
        }
        return choices;
    }

    let mut choices = Vec::<ModelProviderChoice>::new();
    for model in projection.stock_models.iter() {
        if let Some(provider) = choices
            .iter_mut()
            .find(|provider| provider.id == model.provider)
        {
            provider.model_count += 1;
        } else {
            choices.push(ModelProviderChoice {
                id: model.provider.clone(),
                name: provider_display_name(&model.provider),
                model_count: 1,
            });
        }
    }
    choices
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelChoice {
    pub(super) identity: ModelIdentity,
    pub(super) name: String,
    pub(super) provider_name: String,
    pub(super) context_window: u64,
}

pub(super) fn model_choices(
    projection: &ModelRuntimeProjection,
    provider: Option<&str>,
    query: &str,
) -> Vec<ModelChoice> {
    if let Some(catalog) = projection.catalog.as_ref() {
        return catalog
            .models
            .iter()
            .filter(|model| {
                model.available
                    && provider.is_none_or(|provider| model.identity.provider == provider)
                    && model.search_matches(query)
            })
            .take(64)
            .map(|model| ModelChoice {
                identity: model.identity.clone(),
                name: model.name.clone(),
                provider_name: catalog
                    .providers
                    .iter()
                    .find(|provider| provider.id == model.identity.provider)
                    .map(|provider| provider.name.clone())
                    .unwrap_or_else(|| provider_display_name(&model.identity.provider)),
                context_window: model.context_window,
            })
            .collect();
    }

    projection
        .stock_models
        .iter()
        .filter(|model| {
            provider.is_none_or(|provider| model.provider == provider)
                && stock_model_matches(model, query)
        })
        .take(64)
        .map(|model| ModelChoice {
            identity: ModelIdentity {
                provider: model.provider.clone(),
                id: model.id.clone(),
            },
            name: model.name.clone(),
            provider_name: provider_display_name(&model.provider),
            context_window: model.context_window,
        })
        .collect()
}

fn stock_model_matches(model: &ModelSummary, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || model.name.to_lowercase().contains(&query)
        || model.provider.to_lowercase().contains(&query)
        || model.id.to_lowercase().contains(&query)
}

fn provider_display_name(provider: &str) -> String {
    provider
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let first = first.to_uppercase().collect::<String>();
            format!("{first}{}", chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn provider_mark(id: &str, name: &str) -> String {
    let known = match id {
        "amazon-bedrock" => Some("AWS"),
        "anthropic" | "ant-ling" => Some("AN"),
        "azure-openai-responses" | "openai" | "openai-codex" => Some("OA"),
        "cerebras" => Some("CE"),
        "cloudflare-workers-ai" => Some("CF"),
        "deepseek" => Some("DS"),
        "github-copilot" => Some("GH"),
        "google" | "google-vertex" => Some("G"),
        "groq" => Some("GQ"),
        "huggingface" => Some("HF"),
        "mistral" => Some("MI"),
        "nvidia" => Some("NV"),
        "openrouter" => Some("OR"),
        "vercel-ai-gateway" => Some("V"),
        "xai" => Some("xAI"),
        _ => None,
    };
    if let Some(mark) = known {
        return mark.to_owned();
    }

    let words = name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.len() > 1 {
        return words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .flat_map(char::to_uppercase)
            .collect();
    }
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

pub(super) fn thinking_select_sheet(
    projection: &ModelRuntimeProjection,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let levels = thinking_choices(projection);
    let can_change = projection.model_change_policy == ModelChangePolicy::Allowed;
    let active = projection
        .effective_thinking
        .or(projection.active_thinking)
        .or(projection.requested_thinking);

    popup_sheet()
        .id("thinking-select-sheet")
        .h(px(168.0))
        .child(popup_sheet_header("Thinking", "close-thinking-select", cx))
        .when(!can_change, |sheet| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::panel())
                    .border_b_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Settle stream to change"),
            )
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(controls::scroll_wheel_capture(scroll))
                .child(
                    div()
                        .id("thinking-select-scroll")
                        .size_full()
                        .bg(theme::panel())
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .scrollbar_width(px(6.0))
                        .children(levels.into_iter().map(|level| {
                            let selected = active == Some(level);
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "thinking-select-{level:?}"
                                )))
                                .h(px(28.0))
                                .px(px(8.0))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .border_b_1()
                                .border_color(theme::panel_hover())
                                .bg(if selected {
                                    theme::panel_lift()
                                } else {
                                    theme::panel()
                                })
                                .text_color(if !can_change {
                                    theme::smoke()
                                } else if selected {
                                    theme::bone()
                                } else {
                                    theme::ash()
                                })
                                .when(can_change && !selected, |row| {
                                    row.tab_index(0)
                                        .cursor_pointer()
                                        .hover(|row| {
                                            row.bg(theme::panel_lift()).text_color(theme::bone())
                                        })
                                        .active(|row| row.bg(theme::panel_hover()))
                                        .focus(|row| row.bg(theme::panel_lift()))
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.set_thinking(level, window, cx)
                                        }))
                                })
                                .child(
                                    div()
                                        .font_family(theme::main())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .font_weight(if selected {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        })
                                        .child(level.label()),
                                )
                                .when(selected, |row| {
                                    row.child(
                                        div()
                                            .w(px(5.0))
                                            .h(px(5.0))
                                            .rounded_full()
                                            .bg(theme::data())
                                            .flex_shrink_0(),
                                    )
                                })
                        })),
                ),
        )
}

pub(super) fn thinking_choices(projection: &ModelRuntimeProjection) -> Vec<ThinkingLevel> {
    let Some(active) = projection.active_model.as_ref() else {
        return vec![ThinkingLevel::Off];
    };
    if let Some(levels) = projection
        .catalog
        .as_ref()
        .and_then(|catalog| catalog.model(active))
        .map(|model| model.supported_thinking.clone())
    {
        return levels;
    }

    projection
        .stock_models
        .iter()
        .find(|model| model.provider == active.provider && model.id == active.id)
        .map(|model| {
            model
                .supported_thinking
                .iter()
                .copied()
                .map(|level| match level {
                    crate::state::runtime::RuntimeThinkingLevel::Off => ThinkingLevel::Off,
                    crate::state::runtime::RuntimeThinkingLevel::Minimal => ThinkingLevel::Minimal,
                    crate::state::runtime::RuntimeThinkingLevel::Low => ThinkingLevel::Low,
                    crate::state::runtime::RuntimeThinkingLevel::Medium => ThinkingLevel::Medium,
                    crate::state::runtime::RuntimeThinkingLevel::High => ThinkingLevel::High,
                    crate::state::runtime::RuntimeThinkingLevel::Xhigh => ThinkingLevel::Xhigh,
                    crate::state::runtime::RuntimeThinkingLevel::Max => ThinkingLevel::Max,
                })
                .collect()
        })
        .unwrap_or_else(|| vec![ThinkingLevel::Off])
}

fn providers_settings(
    projection: &ModelRuntimeProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let providers = projection
        .catalog
        .as_ref()
        .map(|catalog| catalog.providers.clone())
        .unwrap_or_default();
    div()
        .id("provider-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .scrollbar_width(px(theme::SCROLLBAR))
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(controls::panel_note(
            "Pi owns credentials. The GUI only hosts provider prompts and never stores secret values in catalog state.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .when(providers.is_empty(), |list| {
                    list.child(controls::empty_list_note(
                        "No provider catalog is available. Connect to Pi or refresh catalogs.",
                    ))
                })
                .children(providers.into_iter().map(|provider| {
                    let provider_id = provider.id.clone();
                    let auth_busy = projection.auth.is_some();
                    let status = if provider.auth.configured {
                        provider
                            .auth
                            .source
                            .map(|source| source.label())
                            .unwrap_or("Configured")
                    } else {
                        "Not configured"
                    };
                    div()
                        .px(px(12.0))
                        .py(px(12.0))
                        .border_b_1()
                        .border_color(theme::edge_soft())
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .justify_between()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::bone())
                                        .child(provider.name),
                                )
                                .child(controls::meta_text(format!(
                                    "{status} · {}/{} models",
                                    provider.available_model_count, provider.model_count
                                ))),
                        )
                        .when_some(provider.refresh_error, |row, error| {
                            row.child(controls::panel_note(error, controls::ControlTone::Danger))
                        })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(6.0))
                                .children(provider.auth_methods.into_iter().map(|method| {
                                    let id = provider_id.clone();
                                    controls::chip_button(
                                        gpui::SharedString::from(format!(
                                            "login-{}-{method:?}",
                                            id
                                        )),
                                        method.label(),
                                        false,
                                        !auth_busy,
                                        Box::new(cx.listener(move |view, _, window, cx| {
                                            view.login_provider(id.clone(), method, window, cx)
                                        })),
                                    )
                                }))
                                .when(provider.auth.configured, |buttons| {
                                    let id = provider_id.clone();
                                    buttons.child(controls::chip_button(
                                        gpui::SharedString::from(format!("logout-{id}")),
                                        "Log out",
                                        false,
                                        !auth_busy,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.logout_provider(id.clone(), cx)
                                        })),
                                    ))
                                }),
                        )
                })),
        )
        )
}

pub(super) struct ProviderAuthModalParams<'a> {
    pub(super) auth: &'a AuthFlow,
    pub(super) provider_name: &'a str,
    pub(super) auth_input: &'a Entity<Composer>,
    pub(super) auth_secret: &'a Entity<Composer>,
    pub(super) focus: &'a FocusHandle,
    pub(super) browser_retry_url: Option<&'a str>,
    pub(super) browser_feedback: Option<(&'a str, controls::ControlTone)>,
    pub(super) provider_feedback: Option<&'a str>,
}

pub(super) fn provider_auth_modal(
    params: ProviderAuthModalParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ProviderAuthModalParams {
        auth,
        provider_name,
        auth_input,
        auth_secret,
        focus,
        browser_retry_url,
        browser_feedback,
        provider_feedback,
    } = params;
    let browser_retry_url = browser_retry_url.filter(|_| {
        !matches!(
            &auth.stage,
            AuthStage::Browser { .. } | AuthStage::DeviceCode { .. }
        )
    });
    let stage = match &auth.stage {
        AuthStage::Starting => controls::panel_note(
            "Starting provider authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
        AuthStage::Info { message, links } => div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(controls::panel_note(
                message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(links.iter().cloned().enumerate().map(|(index, link)| {
                let url = link.url;
                controls::quiet_button(
                    gpui::SharedString::from(format!("auth-info-link-{index}")),
                    link.label
                        .unwrap_or_else(|| "Open provider page".to_owned()),
                    true,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.open_provider_auth_url(url.clone(), cx)
                    })),
                )
            }))
            .into_any_element(),
        AuthStage::Browser { url, instructions } => {
            let open_url = url.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(controls::panel_note(
                    instructions
                        .clone()
                        .unwrap_or_else(|| "Finish authentication in your browser.".to_owned()),
                    controls::ControlTone::Normal,
                ))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!("Destination: {}", provider_auth_destination(url))),
                )
                .child(controls::quiet_button(
                    "open-provider-auth-url",
                    "Open browser again",
                    true,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.open_provider_auth_url(open_url.clone(), cx)
                    })),
                ))
                .child(controls::meta_text("Enter reopens browser · Esc cancels"))
                .into_any_element()
        }
        AuthStage::DeviceCode {
            user_code,
            verification_uri,
            expires_in_seconds,
        } => {
            let open_url = verification_uri.clone();
            let copy_code = user_code.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(controls::panel_note(
                    "Enter this one-time code on the provider verification page.",
                    controls::ControlTone::Normal,
                ))
                .child(
                    div()
                        .px(px(14.0))
                        .py(px(12.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(theme::canvas())
                        .border_1()
                        .border_color(theme::edge_hard())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TITLE))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::bone())
                                .child(user_code.clone()),
                        )
                        .when_some(*expires_in_seconds, |row, seconds| {
                            row.child(controls::meta_text(format!("Expires in {seconds}s")))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(controls::quiet_button(
                            "copy-provider-device-code",
                            "Copy code",
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.copy_provider_auth_code(copy_code.clone(), cx)
                            })),
                        ))
                        .child(controls::quiet_button(
                            "open-device-code-url",
                            "Open verification page",
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.open_provider_auth_url(open_url.clone(), cx)
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!(
                            "Destination: {}",
                            provider_auth_destination(verification_uri)
                        )),
                )
                .child(controls::meta_text(
                    "Enter opens page · C copies code · Esc cancels",
                ))
                .into_any_element()
        }
        AuthStage::Progress { message } => {
            controls::panel_note(message.clone(), controls::ControlTone::Normal).into_any_element()
        }
        AuthStage::Prompt(prompt) if prompt.kind == AuthPromptKind::Select => div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(
                prompt
                    .options
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, option)| {
                        let prompt = prompt.clone();
                        let value = option.id;
                        controls::action_row(
                            gpui::SharedString::from(format!("auth-select-{value}")),
                            format!("{}. {}", index + 1, option.label),
                            option.description.unwrap_or_default(),
                            true,
                            controls::ControlTone::Normal,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.answer_auth_select(prompt.clone(), value.clone(), cx)
                            })),
                        )
                    }),
            )
            .child(controls::meta_text("Press 1-9 to choose · Esc cancels"))
            .into_any_element(),
        AuthStage::Prompt(prompt) => div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .child(if prompt.kind == AuthPromptKind::Secret {
                auth_secret.clone().into_any_element()
            } else {
                auth_input.clone().into_any_element()
            })
            .child(
                div()
                    .font_family(theme::mono())
                    .text_size(theme::text_size(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Enter submits · Esc cancels"),
            )
            .into_any_element(),
        AuthStage::Cancelling => controls::panel_note(
            "Cancelling authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
    };

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09e6))
        .flex()
        .items_center()
        .justify_center()
        .p(px(18.0))
        .track_focus(focus)
        .tab_index(0)
        .on_key_down(cx.listener(RootView::on_provider_auth_key_down))
        .child(
            div()
                .id("provider-auth-modal")
                .w_full()
                .max_w(px(620.0))
                .max_h(px(640.0))
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .p(px(18.0))
                .rounded(px(theme::RADIUS))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::edge_hard())
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap(px(14.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_TITLE))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(format!("Authenticate {provider_name}")),
                                )
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .text_color(theme::smoke())
                                        .child(auth.method.label()),
                                ),
                        )
                        .child(controls::chrome_action(
                            "cancel-provider-auth",
                            "Cancel · Esc",
                            !matches!(auth.stage, AuthStage::Cancelling),
                            Box::new(cx.listener(|view, _, _, cx| {
                                view.cancel_provider_auth(cx)
                            })),
                        )),
                )
                .child(stage)
                .when_some(browser_retry_url, |modal, retry_url| {
                    let retry_url = retry_url.to_owned();
                    modal.child(controls::quiet_button(
                        "retry-provider-auth-browser",
                        "Open browser again",
                        true,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            view.open_provider_auth_url(retry_url.clone(), cx)
                        })),
                    ))
                })
                .when_some(browser_feedback, |modal, (message, tone)| {
                    modal.child(controls::panel_note(message.to_owned(), tone))
                })
                .when_some(provider_feedback, |modal, message| {
                    modal.child(controls::panel_note(
                        message.to_owned(),
                        controls::ControlTone::Danger,
                    ))
                })
                .child(controls::panel_note(
                    "Pi handles and stores provider credentials. This app only displays Pi's prompts.",
                    controls::ControlTone::Normal,
                )),
        )
}

fn provider_auth_destination(url: &str) -> &str {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme.split(['/', '?', '#']).next().unwrap_or(url)
}

fn models_settings(
    projection: &ModelRuntimeProjection,
    search: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft().to_owned();
    let (models, defaults) = projection
        .catalog
        .as_ref()
        .map(|catalog| {
            (
                catalog
                    .models
                    .iter()
                    .filter(|model| model.search_matches(&query))
                    .take(200)
                    .cloned()
                    .collect::<Vec<_>>(),
                catalog.defaults.clone(),
            )
        })
        .unwrap_or_default();
    div()
        .id("models-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .scrollbar_width(px(theme::SCROLLBAR))
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(search.clone())
        .child(controls::panel_note(
            "Defaults and cycle order are saved by Pi for future sessions. Switch the active model from the prompt box.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .when(models.is_empty(), |list| {
                    list.child(controls::empty_list_note("No models match this search."))
                })
                .children(
                    models
                        .into_iter()
                        .map(|model| model_settings_row(model, defaults.clone(), cx)),
                ),
        )
        )
}

fn model_settings_row(
    model: ModelCatalogEntry,
    defaults: crate::model_runtime::ModelDefaults,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let identity = model.identity.clone();
    let is_default = defaults.model.as_ref() == Some(&identity);
    let in_scope = defaults.scoped_models.contains(&identity);
    let default_identity = identity.clone();
    let scope_identity = identity.clone();
    let availability = if model.available {
        "Available"
    } else {
        "Unavailable"
    };
    div()
        .px(px(12.0))
        .py(px(11.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .font_family(theme::sans())
                                .text_size(theme::text_size(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(model.name),
                        )
                        .child(controls::meta_text(format!(
                            "{} · {} · {} ctx · max {}",
                            identity.display(),
                            model.api,
                            compact_count(model.context_window),
                            compact_count(model.max_tokens),
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .flex_shrink_0()
                        .when(is_default, |row| {
                            row.child(controls::chip_button(
                                gpui::SharedString::from(format!(
                                    "badge-default-{}-{}",
                                    identity.provider, identity.id
                                )),
                                "Default",
                                true,
                                false,
                                Box::new(|_, _, _| {}),
                            ))
                        })
                        .when(in_scope, |row| {
                            row.child(controls::chip_button(
                                gpui::SharedString::from(format!(
                                    "badge-cycle-{}-{}",
                                    identity.provider, identity.id
                                )),
                                "Cycle",
                                true,
                                false,
                                Box::new(|_, _, _| {}),
                            ))
                        }),
                ),
        )
        .child(controls::meta_text(format!(
            "{availability} · {}",
            model.pricing.label()
        )))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .child(controls::chip_button(
                    gpui::SharedString::from(format!(
                        "default-{}-{}",
                        identity.provider, identity.id
                    )),
                    if is_default {
                        "Default model"
                    } else {
                        "Set as default"
                    },
                    is_default,
                    !is_default,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.set_default_model(default_identity.clone(), cx)
                    })),
                ))
                .child(controls::chip_button(
                    gpui::SharedString::from(format!(
                        "scope-{}-{}",
                        identity.provider, identity.id
                    )),
                    if in_scope {
                        "Remove from cycle"
                    } else {
                        "Add to cycle"
                    },
                    in_scope,
                    true,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.toggle_model_scope(scope_identity.clone(), cx)
                    })),
                )),
        )
        .into_any_element()
}

fn thinking_settings(
    projection: &ModelRuntimeProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let defaults = projection
        .catalog
        .as_ref()
        .map(|catalog| catalog.defaults.clone());
    let session_label = format!(
        "Session: requested {} · effective {}",
        projection
            .requested_thinking
            .or(projection.active_thinking)
            .map(ThinkingLevel::label)
            .unwrap_or("Unknown"),
        projection
            .effective_thinking
            .or(projection.active_thinking)
            .map(ThinkingLevel::label)
            .unwrap_or("Unknown")
    );
    div()
        .id("thinking-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(controls::panel_note(
            "Change the active session thinking level from the prompt box. This page only sets Pi's default for future sessions. Levels are discrete and model-specific; unsupported values are never invented.",
            controls::ControlTone::Normal,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(theme::edge_soft())
                .bg(theme::panel())
                .child(controls::section_label("Current session"))
                .child(controls::meta_text(session_label))
                .when_some(projection.clamp_notice.clone(), |panel, notice| {
                    panel.child(controls::panel_note(notice, controls::ControlTone::Normal))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(controls::section_label("Default for new sessions"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(ThinkingLevel::ALL.into_iter().map(|level| {
                            let selected =
                                defaults.as_ref().and_then(|value| value.thinking) == Some(level);
                            controls::chip_button(
                                gpui::SharedString::from(format!("default-thinking-{level:?}")),
                                level.label(),
                                selected,
                                !selected,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_default_thinking(level, cx)
                                })),
                            )
                        })),
                ),
        )
        )
}

fn pi_settings(
    projection: &ModelRuntimeProjection,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let Some(settings) = projection
        .catalog
        .as_ref()
        .map(|catalog| catalog.settings.clone())
    else {
        return div()
            .flex_1()
            .min_h_0()
            .px(px(18.0))
            .py(px(18.0))
            .child(controls::panel_note(
                "Pi settings are unavailable until the model bridge finishes loading.",
                controls::ControlTone::Normal,
            ))
            .into_any_element();
    };

    div()
        .flex_1()
        .min_h_0()
        .relative()
        .child(controls::scroll_wheel_capture(scroll))
        .child(
            div()
                .id("pi-settings-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .max_w(px(980.0))
                        .mx_auto()
                        .px(px(18.0))
                        .pt(px(14.0))
                        .pb(px(28.0))
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .child(controls::panel_note(
                            "These controls write Pi's global settings through SettingsManager. Project-local overrides remain owned by trusted Pi projects and are never rewritten here.",
                            controls::ControlTone::Normal,
                        ))
                        .child(pi_settings_group(
                            "Message delivery",
                            "How Pi queues messages and chooses its provider transport.",
                            vec![
                                pi_select_setting_row(
                                    "Steering delivery",
                                    "Messages sent during a run can be delivered together or one at a time.",
                                    "steeringMode",
                                    &settings.steering_mode,
                                    &[
                                        ("One at a time", "one-at-a-time"),
                                        ("All queued", "all"),
                                    ],
                                    cx,
                                ),
                                pi_select_setting_row(
                                    "Follow-up delivery",
                                    "Messages queued for the next turn can be delivered together or sequentially.",
                                    "followUpMode",
                                    &settings.follow_up_mode,
                                    &[
                                        ("One at a time", "one-at-a-time"),
                                        ("All queued", "all"),
                                    ],
                                    cx,
                                ),
                                pi_select_setting_row(
                                    "Provider transport",
                                    "Choose Pi's preferred transport when a provider supports more than one.",
                                    "transport",
                                    &settings.transport,
                                    &[
                                        ("Auto", "auto"),
                                        ("SSE", "sse"),
                                        ("WebSocket", "websocket"),
                                        ("WS cached", "websocket-cached"),
                                    ],
                                    cx,
                                ),
                            ],
                        ))
                        .child(pi_settings_group(
                            "Agent behavior",
                            "Recovery, compaction, and transcript behavior controlled by Pi.",
                            vec![
                                pi_toggle_setting_row(
                                    "Automatic compaction",
                                    "Allow Pi to compact context when it approaches configured limits.",
                                    "compaction.enabled",
                                    settings.compaction_enabled,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Automatic retry",
                                    "Retry transient agent-level failures with Pi's configured backoff.",
                                    "retry.enabled",
                                    settings.retry_enabled,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Hide thinking blocks",
                                    "Keep reasoning blocks out of Pi's rendered transcript output.",
                                    "hideThinkingBlock",
                                    settings.hide_thinking_block,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Cache-miss notices",
                                    "Show transcript notices for significant prompt-cache misses.",
                                    "showCacheMissNotices",
                                    settings.show_cache_miss_notices,
                                    cx,
                                ),
                            ],
                        ))
                        .child(pi_settings_group(
                            "Interaction",
                            "Navigation defaults and editor density for Pi-managed interfaces.",
                            vec![
                                pi_select_setting_row(
                                    "Default project trust",
                                    "Fallback used by non-interactive Pi modes when no saved trust decision applies.",
                                    "defaultProjectTrust",
                                    &settings.default_project_trust,
                                    &[("Ask", "ask"), ("Always", "always"), ("Never", "never")],
                                    cx,
                                ),
                                pi_select_setting_row(
                                    "Double escape",
                                    "Choose the action Pi runs when Escape is pressed twice with an empty editor.",
                                    "doubleEscapeAction",
                                    &settings.double_escape_action,
                                    &[("Tree", "tree"), ("Fork", "fork"), ("None", "none")],
                                    cx,
                                ),
                                pi_select_setting_row(
                                    "Tree filter",
                                    "Default message filter when Pi opens its session tree.",
                                    "treeFilterMode",
                                    &settings.tree_filter_mode,
                                    &[
                                        ("Default", "default"),
                                        ("No tools", "no-tools"),
                                        ("User only", "user-only"),
                                        ("Labeled", "labeled-only"),
                                        ("All", "all"),
                                    ],
                                    cx,
                                ),
                                pi_stepper_setting_row(
                                    "Autocomplete rows",
                                    "Maximum visible entries in Pi's autocomplete menu.",
                                    "autocompleteMaxVisible",
                                    settings.autocomplete_max_visible,
                                    3,
                                    20,
                                    cx,
                                ),
                            ],
                        ))
                        .child(pi_settings_group(
                            "Display",
                            "Terminal-facing presentation settings retained for Pi compatibility.",
                            vec![
                                pi_toggle_setting_row(
                                    "Quiet startup",
                                    "Hide Pi's startup header.",
                                    "quietStartup",
                                    settings.quiet_startup,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Condensed changelog",
                                    "Show the compact changelog after Pi updates.",
                                    "collapseChangelog",
                                    settings.collapse_changelog,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Hardware cursor",
                                    "Keep the terminal cursor visible for IME support in Pi's TUI.",
                                    "showHardwareCursor",
                                    settings.show_hardware_cursor,
                                    cx,
                                ),
                                pi_stepper_setting_row(
                                    "Editor padding",
                                    "Horizontal padding used by Pi's input editor.",
                                    "editorPaddingX",
                                    settings.editor_padding_x,
                                    0,
                                    3,
                                    cx,
                                ),
                                pi_stepper_setting_row(
                                    "Output padding",
                                    "Horizontal padding around Pi transcript messages.",
                                    "outputPad",
                                    settings.output_pad,
                                    0,
                                    1,
                                    cx,
                                ),
                            ],
                        ))
                        .child(pi_settings_group(
                            "Images and resources",
                            "Control image handling and whether installed skills become slash commands.",
                            vec![
                                pi_toggle_setting_row(
                                    "Show terminal images",
                                    "Render inline images when the terminal supports them.",
                                    "terminal.showImages",
                                    settings.show_images,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Auto-resize images",
                                    "Resize oversized images before sending them to providers.",
                                    "images.autoResize",
                                    settings.image_auto_resize,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Block image input",
                                    "Prevent images from being sent to language-model providers.",
                                    "images.blockImages",
                                    settings.block_images,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Clear on terminal shrink",
                                    "Clear empty terminal rows after Pi output becomes shorter.",
                                    "terminal.clearOnShrink",
                                    settings.clear_on_shrink,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Skill slash commands",
                                    "Register installed Pi skills as slash commands.",
                                    "enableSkillCommands",
                                    settings.enable_skill_commands,
                                    cx,
                                ),
                            ],
                        ))
                        .child(pi_settings_group(
                            "Privacy",
                            "Pi's independent telemetry preferences. Update checks are configured separately.",
                            vec![
                                pi_toggle_setting_row(
                                    "Install telemetry",
                                    "Allow Pi's anonymous install and update version ping.",
                                    "enableInstallTelemetry",
                                    settings.enable_install_telemetry,
                                    cx,
                                ),
                                pi_toggle_setting_row(
                                    "Analytics",
                                    "Opt in to Pi analytics and create a tracking identifier if needed.",
                                    "enableAnalytics",
                                    settings.enable_analytics,
                                    cx,
                                ),
                            ],
                        )),
                ),
        )
        .into_any_element()
}

fn pi_settings_group(
    title: &'static str,
    detail: &'static str,
    rows: Vec<gpui::AnyElement>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(controls::section_label(title))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .line_height(gpui::relative(1.35))
                        .text_color(theme::smoke())
                        .child(detail),
                ),
        )
        .child(controls::divider_list().children(rows))
        .into_any_element()
}

fn pi_setting_row(
    label: &'static str,
    detail: &'static str,
    control: gpui::AnyElement,
) -> gpui::AnyElement {
    div()
        .min_h(px(56.0))
        .px(px(12.0))
        .py(px(9.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(18.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .max_w(px(520.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone_dim())
                        .child(label),
                )
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .line_height(gpui::relative(1.35))
                        .text_color(theme::smoke())
                        .child(detail),
                ),
        )
        .child(
            div()
                .min_w_0()
                .w_full()
                .max_w(px(440.0))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(control),
        )
        .into_any_element()
}

fn pi_toggle_setting_row(
    label: &'static str,
    detail: &'static str,
    key: &'static str,
    enabled: bool,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let id = key.replace('.', "-");
    let on_id = gpui::SharedString::from(format!("pi-setting-{id}-on"));
    let off_id = gpui::SharedString::from(format!("pi-setting-{id}-off"));
    let control = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(controls::chip_button(
            on_id,
            "On",
            enabled,
            !enabled,
            Box::new(cx.listener(move |view, _, _, cx| {
                view.set_pi_setting(key, serde_json::Value::Bool(true), cx)
            })),
        ))
        .child(controls::chip_button(
            off_id,
            "Off",
            !enabled,
            enabled,
            Box::new(cx.listener(move |view, _, _, cx| {
                view.set_pi_setting(key, serde_json::Value::Bool(false), cx)
            })),
        ))
        .into_any_element();
    pi_setting_row(label, detail, control)
}

fn pi_select_setting_row(
    label: &'static str,
    detail: &'static str,
    key: &'static str,
    current: &str,
    options: &'static [(&'static str, &'static str)],
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let id = key.replace('.', "-");
    let control = div()
        .w_full()
        .flex()
        .flex_row()
        .justify_end()
        .gap(px(5.0))
        .children(options.iter().map(|(option_label, value)| {
            let option_label = *option_label;
            let value = *value;
            let selected = current == value;
            controls::chip_button(
                gpui::SharedString::from(format!("pi-setting-{id}-{value}")),
                option_label,
                selected,
                !selected,
                Box::new(cx.listener(move |view, _, _, cx| {
                    view.set_pi_setting(key, serde_json::Value::String(value.to_owned()), cx)
                })),
            )
        }))
        .into_any_element();
    pi_setting_row(label, detail, control)
}

fn pi_stepper_setting_row(
    label: &'static str,
    detail: &'static str,
    key: &'static str,
    value: u8,
    minimum: u8,
    maximum: u8,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let id = key.replace('.', "-");
    let decrease = value.saturating_sub(1).max(minimum);
    let increase = value.saturating_add(1).min(maximum);
    let control = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(controls::chip_button(
            gpui::SharedString::from(format!("pi-setting-{id}-decrease")),
            "−",
            false,
            value > minimum,
            Box::new(cx.listener(move |view, _, _, cx| {
                view.set_pi_setting(key, serde_json::Value::from(decrease), cx)
            })),
        ))
        .child(
            div()
                .min_w(px(38.0))
                .h(px(28.0))
                .px(px(9.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(theme::edge_soft())
                .bg(theme::canvas())
                .flex()
                .items_center()
                .justify_center()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_MONO_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::bone_dim())
                .child(value.to_string()),
        )
        .child(controls::chip_button(
            gpui::SharedString::from(format!("pi-setting-{id}-increase")),
            "+",
            false,
            value < maximum,
            Box::new(cx.listener(move |view, _, _, cx| {
                view.set_pi_setting(key, serde_json::Value::from(increase), cx)
            })),
        ))
        .into_any_element();
    pi_setting_row(label, detail, control)
}

fn usage_settings(projection: &ModelRuntimeProjection) -> impl IntoElement {
    let usage = &projection.usage;
    let context = match (usage.context_tokens, usage.context_window) {
        (Some(tokens), Some(window)) => {
            format!("{} / {}", compact_count(tokens), compact_count(window))
        }
        _ => "Unknown until Pi reports current context".to_owned(),
    };
    let cost = usage.estimated_cost.map_or_else(
        || "Unknown".to_owned(),
        |cost| {
            if usage.pricing_known {
                format!("${cost:.4} estimated")
            } else {
                format!("${cost:.4} estimated · pricing may be unavailable")
            }
        },
    );
    div()
        .id("usage-settings-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().w_full().px(px(18.0))
        .pb(px(22.0))
        .pt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(controls::panel_note(
            "Current context is nullable and separate from lifetime session totals. Cost is an estimate; zero catalog rates mean unpriced, not free.",
            controls::ControlTone::Normal,
        ))
        .child(
            controls::divider_list()
                .child(controls::metric_row("Current context", context))
                .child(controls::metric_row(
                    "Lifetime input",
                    optional_count(usage.input_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime output",
                    optional_count(usage.output_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime cache read",
                    optional_count(usage.cache_read_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime cache write",
                    optional_count(usage.cache_write_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime reasoning",
                    optional_count(usage.reasoning_tokens),
                ))
                .child(controls::metric_row(
                    "Lifetime total",
                    optional_count(usage.total_tokens),
                ))
                .child(controls::metric_row("Estimated cost", cost)),
        )
        )
}

fn resource_center_settings(
    projection: &ResourceCenterProjection,
    scope_filter: ResourceScopeFilter,
    state_filter: ResourceStateFilter,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let snapshot = projection.snapshot.as_ref();
    let items = snapshot
        .map(|snapshot| {
            snapshot
                .items
                .iter()
                .filter(|item| scope_filter.matches(item) && state_filter.matches(item))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let summary = snapshot.map(|snapshot| {
        let loaded = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Loaded)
            .count();
        let disabled = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Disabled)
            .count();
        let errors = snapshot
            .items
            .iter()
            .filter(|item| item.state == ResourceLoadState::Error)
            .count();
        format!(
            "{} total · {loaded} loaded · {disabled} disabled · {errors} error{}",
            snapshot.items.len(),
            if errors == 1 { "" } else { "s" }
        )
    });

    div()
        .id("resource-center-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .px(px(18.0))
                .pb(px(22.0))
                .pt(px(14.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(controls::panel_note(
                    snapshot
                        .map(|snapshot| snapshot.project_trust_reason.clone())
                        .unwrap_or_else(|| {
                            "Loading the capability-gated Pi resource inventory…".to_owned()
                        }),
                    controls::ControlTone::Normal,
                ))
                .when_some(summary, |panel, summary| {
                    panel.child(controls::meta_text(summary))
                })
                .child(
                    div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                        [
                            (ResourceScopeFilter::All, "All scopes"),
                            (ResourceScopeFilter::Global, "Global"),
                            (ResourceScopeFilter::Project, "Project"),
                            (ResourceScopeFilter::Package, "Package"),
                        ]
                        .into_iter()
                        .map(|(filter, label)| {
                            controls::chip_button(
                                gpui::SharedString::from(format!("resource-scope-{filter:?}")),
                                label,
                                scope_filter == filter,
                                true,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_resource_scope_filter(filter, cx)
                                })),
                            )
                        }),
                    ),
                )
                .child(
                    div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                        [
                            (ResourceStateFilter::All, "All states"),
                            (ResourceStateFilter::Loaded, "Loaded"),
                            (ResourceStateFilter::Disabled, "Disabled"),
                            (ResourceStateFilter::Error, "Errors"),
                        ]
                        .into_iter()
                        .map(|(filter, label)| {
                            controls::chip_button(
                                gpui::SharedString::from(format!("resource-state-{filter:?}")),
                                label,
                                state_filter == filter,
                                true,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_resource_state_filter(filter, cx)
                                })),
                            )
                        }),
                    ),
                )
                .when_some(snapshot, |panel, snapshot| {
                    panel
                        .child(
                            controls::divider_list()
                                .child(controls::metric_row(
                                    "Skill commands",
                                    if snapshot.settings.enable_skill_commands {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    },
                                ))
                                .child(controls::metric_row(
                                    "Pi theme",
                                    snapshot
                                        .settings
                                        .theme
                                        .clone()
                                        .unwrap_or_else(|| "Default".to_owned()),
                                ))
                                .child(controls::metric_row(
                                    "Default project trust",
                                    snapshot.settings.default_project_trust.clone(),
                                )),
                        )
                        .child(controls::panel_note(
                            snapshot.package_mutations.reason.clone(),
                            controls::ControlTone::Danger,
                        ))
                })
                .when(items.is_empty(), |panel| {
                    panel.child(controls::empty_list_note(match projection.phase {
                        ResourcePhase::Loading | ResourcePhase::Refreshing => "Loading resources…",
                        ResourcePhase::Failed(_) => "Resource inventory unavailable.",
                        ResourcePhase::Ready => "No resources match these filters.",
                    }))
                })
                .children(items.into_iter().map(resource_center_row))
                .when_some(snapshot, |panel, snapshot| {
                    panel.children(snapshot.diagnostics.iter().cloned().map(|diagnostic| {
                        controls::panel_note(diagnostic, controls::ControlTone::Normal)
                    }))
                }),
        )
}

fn resource_center_row(item: crate::resource_center::ResourceItem) -> impl IntoElement {
    let state_color = match item.state {
        ResourceLoadState::Loaded => theme::live(),
        ResourceLoadState::Disabled => theme::ash(),
        ResourceLoadState::Error => theme::error(),
    };
    let active = item.active.map(|active| {
        if active {
            " · active tool"
        } else {
            " · inactive tool"
        }
    });
    let package_flags = match (item.pinned, item.filtered) {
        (Some(true), Some(true)) => " · pinned · filtered",
        (Some(true), _) => " · pinned",
        (_, Some(true)) => " · filtered",
        _ => "",
    };
    let metadata = format!(
        "{} · {} · {} · {}{}{}",
        item.kind.label(),
        item.scope.label(),
        item.state.label(),
        item.trust.label(),
        active.unwrap_or_default(),
        package_flags
    );
    div()
        .id(gpui::SharedString::from(format!("resource-{}", item.id)))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(theme::edge_soft())
        .bg(theme::panel())
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(item.name),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(state_color)
                        .child(item.state.label()),
                ),
        )
        .child(controls::meta_text(metadata))
        .when_some(item.description, |row, description| {
            row.child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_TINY))
                    .line_height(gpui::relative(1.4))
                    .text_color(theme::bone_dim())
                    .child(description),
            )
        })
        .when_some(item.path, |row, path| row.child(controls::meta_text(path)))
        .child(controls::meta_text(format!("Source: {}", item.source)))
        .children(item.diagnostics.into_iter().map(|diagnostic| {
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .line_height(gpui::relative(1.4))
                .text_color(theme::error())
                .child(diagnostic)
        }))
}

fn catalog_phase_note(phase: &CatalogPhase) -> Option<String> {
    match phase {
        CatalogPhase::Loading => Some("Loading cached model catalogs...".to_owned()),
        CatalogPhase::Refreshing => {
            Some("Refreshing provider catalogs; cached models remain visible.".to_owned())
        }
        CatalogPhase::Stale(summary) | CatalogPhase::Failed(summary) => Some(summary.clone()),
        CatalogPhase::Ready => None,
    }
}

fn optional_count(value: Option<u64>) -> String {
    value
        .map(compact_count)
        .unwrap_or_else(|| "Unknown".to_owned())
}

fn compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::provider_auth_destination;

    #[test]
    fn auth_destination_hides_paths_and_query_values() {
        assert_eq!(
            provider_auth_destination("https://provider.example/oauth/start?state=private"),
            "provider.example"
        );
        assert_eq!(
            provider_auth_destination("http://localhost:9876/callback#token"),
            "localhost:9876"
        );
    }
}
