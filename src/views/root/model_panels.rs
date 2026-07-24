use super::shared::{popup_sheet, popup_sheet_header};
use super::*;

pub(super) struct ModelSettingsPanelParams<'a> {
    pub(super) projection: &'a ModelRuntimeProjection,
    pub(super) resources: &'a ResourceCenterProjection,
    pub(super) tab: ModelSettingsTab,
    pub(super) resource_scope_filter: ResourceScopeFilter,
    pub(super) resource_state_filter: ResourceStateFilter,
    pub(super) search: &'a Entity<Composer>,
    pub(super) auth_input: &'a Entity<Composer>,
    pub(super) auth_secret: &'a Entity<Composer>,
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
        auth_input,
        auth_secret,
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
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(if tab == ModelSettingsTab::Resources {
                                            "Resource Center"
                                        } else {
                                            "Model settings"
                                        }),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_TINY))
                                        .text_color(theme::ash())
                                        .child(
                                            if tab == ModelSettingsTab::Resources {
                                                "Audited Pi resources, provenance, trust, load state, and active tools."
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
                                .child(controls::quiet_button(
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
                            (ModelSettingsTab::Usage, "Usage"),
                            (ModelSettingsTab::Resources, "Resources"),
                        ]
                        .into_iter()
                        .map(|(target, label)| {
                            controls::tab_button(
                                gpui::SharedString::from(format!("model-tab-{label}")),
                                label,
                                tab == target,
                                Box::new(cx.listener(move |view, _, _, cx| {
                                    view.set_model_settings_tab(target, cx)
                                })),
                            )
                        }),
                    ),
                ),
        )
        .child(match tab {
            ModelSettingsTab::Providers => {
                providers_settings(projection, auth_input, auth_secret, cx).into_any_element()
            }
            ModelSettingsTab::Models => models_settings(projection, search, cx).into_any_element(),
            ModelSettingsTab::Thinking => thinking_settings(projection, cx).into_any_element(),
            ModelSettingsTab::Usage => usage_settings(projection).into_any_element(),
            ModelSettingsTab::Resources => resource_center_settings(
                resources,
                resource_scope_filter,
                resource_state_filter,
                cx,
            )
            .into_any_element(),
        })
        .when(tab != ModelSettingsTab::Resources, |panel| {
            panel
                .when_some(catalog_phase_note(&projection.phase), |panel, note| {
                    panel.child(controls::panel_footer_status(note))
                })
                .when_some(projection.feedback.clone(), |panel, feedback| {
                    panel.child(controls::panel_footer_status(feedback))
                })
        })
        .when(tab == ModelSettingsTab::Resources, |panel| {
            panel.when_some(resources.feedback.clone(), |panel, feedback| {
                panel.child(controls::panel_footer_status(feedback))
            })
        })
}

pub(super) fn model_switcher_sheet(
    projection: &ModelRuntimeProjection,
    search: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let query = search.read(cx).draft().to_owned();
    let models = model_choices(projection, &query);
    let active = projection.active_model.clone();
    let can_change = projection.model_change_policy == ModelChangePolicy::Allowed;

    popup_sheet()
        .id("model-switcher-sheet")
        .max_h(px(300.0))
        .child(
            div()
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .bg(theme::panel())
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_LABEL))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone_dim())
                        .flex_shrink_0()
                        .child("Model"),
                )
                .child(div().flex_1().min_w_0().child(search.clone()))
                .child(controls::chrome_action(
                    "close-model-switcher",
                    "Close",
                    true,
                    Box::new(cx.listener(|view, _, window, cx| view.close_model_panel(window, cx))),
                )),
        )
        .when(!can_change, |sheet| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .bg(theme::panel())
                    .border_b_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Settle stream to change"),
            )
        })
        .child(
            div()
                .id("model-switcher-scroll")
                .flex_1()
                .min_h_0()
                .bg(theme::panel())
                .overflow_y_scroll()
                .scrollbar_width(px(6.0))
                .when(models.is_empty(), |list| {
                    list.child(
                        div()
                            .px(px(12.0))
                            .py(px(18.0))
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI_SM))
                            .text_color(theme::smoke())
                            .child(match projection.phase {
                                CatalogPhase::Loading => "Loading models…",
                                _ => "No matching models.",
                            }),
                    )
                })
                .children(models.into_iter().map(|model| {
                    let identity = model.identity.clone();
                    let selected = active.as_ref() == Some(&identity);
                    let context = format!("{} ctx", compact_count(model.context_window));
                    let monogram = model
                        .name
                        .chars()
                        .next()
                        .unwrap_or('M')
                        .to_uppercase()
                        .to_string();
                    div()
                        .id(gpui::SharedString::from(format!(
                            "switch-model-{}-{}",
                            identity.provider, identity.id
                        )))
                        .h(px(48.0))
                        .mx(px(6.0))
                        .my(px(2.0))
                        .px(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(if selected {
                            theme::data()
                        } else {
                            theme::edge_soft()
                        })
                        .bg(if selected {
                            theme::data_wash()
                        } else {
                            theme::panel()
                        })
                        .when(can_change && !selected, |row| {
                            let identity = identity.clone();
                            row.tab_index(0)
                                .cursor_pointer()
                                .hover(|row| {
                                    row.bg(theme::panel_lift()).border_color(theme::edge())
                                })
                                .active(|row| row.bg(theme::panel_hover()))
                                .focus(|row| {
                                    row.bg(theme::panel_lift()).border_color(theme::focus())
                                })
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.select_model(identity.clone(), window, cx)
                                }))
                        })
                        .child(
                            div()
                                .size(px(28.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .border_1()
                                .border_color(if selected {
                                    theme::data()
                                } else {
                                    theme::edge_soft()
                                })
                                .bg(if selected {
                                    theme::data_wash()
                                } else {
                                    theme::canvas()
                                })
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_LABEL))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if selected {
                                    theme::data()
                                } else {
                                    theme::ash()
                                })
                                .child(monogram),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(if selected {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::SEMIBOLD
                                        })
                                        .text_color(if selected {
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
                                        .font_family(theme::MONO)
                                        .text_size(px(10.0))
                                        .text_color(theme::smoke())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(identity.provider),
                                ),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(10.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::ash())
                                .flex_shrink_0()
                                .child(context),
                        )
                        .when(selected, |row| {
                            row.child(
                                div()
                                    .px(px(6.0))
                                    .py(px(3.0))
                                    .rounded(px(theme::RADIUS_SM))
                                    .bg(theme::data_wash())
                                    .font_family(theme::SANS)
                                    .text_size(px(9.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::data())
                                    .flex_shrink_0()
                                    .child("Active"),
                            )
                        })
                })),
        )
        .when_some(projection.feedback.clone(), |sheet, feedback| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::panel())
                    .border_t_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::MONO)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(feedback),
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelChoice {
    pub(super) identity: ModelIdentity,
    pub(super) name: String,
    pub(super) context_window: u64,
}

pub(super) fn model_choices(projection: &ModelRuntimeProjection, query: &str) -> Vec<ModelChoice> {
    if let Some(catalog) = projection.catalog.as_ref() {
        return catalog
            .models
            .iter()
            .filter(|model| model.available && model.search_matches(query))
            .take(48)
            .map(|model| ModelChoice {
                identity: model.identity.clone(),
                name: model.name.clone(),
                context_window: model.context_window,
            })
            .collect();
    }

    projection
        .stock_models
        .iter()
        .filter(|model| stock_model_matches(model, query))
        .take(48)
        .map(|model| ModelChoice {
            identity: ModelIdentity {
                provider: model.provider.clone(),
                id: model.id.clone(),
            },
            name: model.name.clone(),
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

pub(super) fn thinking_select_sheet(
    projection: &ModelRuntimeProjection,
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
        .max_h(px(168.0))
        .child(popup_sheet_header("Thinking", "close-thinking-select", cx))
        .when(!can_change, |sheet| {
            sheet.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::panel())
                    .border_b_1()
                    .border_color(theme::panel_hover())
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Settle stream to change"),
            )
        })
        .child(
            div()
                .id("thinking-select-scroll")
                .flex_1()
                .min_h_0()
                .bg(theme::panel())
                .overflow_y_scroll()
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
                                .hover(|row| row.bg(theme::panel_lift()).text_color(theme::bone()))
                                .active(|row| row.bg(theme::panel_hover()))
                                .focus(|row| row.bg(theme::panel_lift()))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.set_thinking(level, window, cx)
                                }))
                        })
                        .child(
                            div()
                                .font_family(theme::CONTROL)
                                .text_size(px(theme::T_TINY))
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
    auth_input: &Entity<Composer>,
    auth_secret: &Entity<Composer>,
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
        .when_some(projection.auth.as_ref(), |panel, auth| {
            panel.child(auth_flow_panel(auth, auth_input, auth_secret, cx))
        })
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
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
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
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.login_provider(id.clone(), method, cx)
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

fn auth_flow_panel(
    auth: &crate::model_runtime::AuthFlow,
    auth_input: &Entity<Composer>,
    auth_secret: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let stage = match &auth.stage {
        AuthStage::Starting => controls::panel_note(
            "Starting provider-owned authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
        AuthStage::Info { message, links } => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(links.iter().cloned().map(|link| {
                let url = link.url;
                controls::quiet_button(
                    gpui::SharedString::from(format!("auth-link-{url}")),
                    link.label
                        .unwrap_or_else(|| "Open provider page".to_owned()),
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                )
            }))
            .into_any_element(),
        AuthStage::Browser { url, instructions } => {
            let url = url.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(controls::panel_note(
                    instructions
                        .clone()
                        .unwrap_or_else(|| "Continue authentication in your browser.".to_owned()),
                    controls::ControlTone::Normal,
                ))
                .child(controls::quiet_button(
                    "open-provider-auth-url",
                    "Open browser",
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                ))
                .into_any_element()
        }
        AuthStage::DeviceCode {
            user_code,
            verification_uri,
            expires_in_seconds,
        } => {
            let url = verification_uri.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(controls::panel_note(
                    format!(
                        "Enter device code {}{}",
                        user_code,
                        expires_in_seconds
                            .map(|seconds| format!(" · expires in {seconds}s"))
                            .unwrap_or_default()
                    ),
                    controls::ControlTone::Normal,
                ))
                .child(controls::quiet_button(
                    "open-device-code-url",
                    "Open verification page",
                    true,
                    Box::new(move |_, _, _| {
                        let _ = crate::services::path_actions::open_provider_auth_url(&url);
                    }),
                ))
                .into_any_element()
        }
        AuthStage::Progress { message } => {
            controls::panel_note(message.clone(), controls::ControlTone::Normal).into_any_element()
        }
        AuthStage::Prompt(prompt) if prompt.kind == AuthPromptKind::Select => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .children(prompt.options.iter().cloned().map(|option| {
                let prompt = prompt.clone();
                let value = option.id;
                controls::action_row(
                    gpui::SharedString::from(format!("auth-select-{value}")),
                    option.label,
                    option.description.unwrap_or_default(),
                    true,
                    controls::ControlTone::Normal,
                    Box::new(cx.listener(move |view, _, _, cx| {
                        view.answer_auth_select(prompt.clone(), value.clone(), cx)
                    })),
                )
            }))
            .into_any_element(),
        AuthStage::Prompt(prompt) => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(controls::panel_note(
                prompt.message.clone(),
                controls::ControlTone::Normal,
            ))
            .child(if prompt.kind == AuthPromptKind::Secret {
                auth_secret.clone().into_any_element()
            } else {
                auth_input.clone().into_any_element()
            })
            .into_any_element(),
        AuthStage::Cancelling => controls::panel_note(
            "Cancelling authentication...",
            controls::ControlTone::Normal,
        )
        .into_any_element(),
    };
    div()
        .p(px(10.0))
        .border_1()
        .border_color(theme::signal())
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(controls::section_label(format!(
            "Authenticate {} · {}",
            auth.provider,
            auth.method.label()
        )))
        .child(stage)
        .child(controls::quiet_button(
            "cancel-provider-auth",
            "Cancel",
            !matches!(auth.stage, AuthStage::Cancelling),
            Box::new(cx.listener(|view, _, _, cx| view.cancel_provider_auth(cx))),
        ))
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
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
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
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
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
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(state_color)
                        .child(item.state.label()),
                ),
        )
        .child(controls::meta_text(metadata))
        .when_some(item.description, |row, description| {
            row.child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_TINY))
                    .line_height(gpui::relative(1.4))
                    .text_color(theme::bone_dim())
                    .child(description),
            )
        })
        .when_some(item.path, |row, path| row.child(controls::meta_text(path)))
        .child(controls::meta_text(format!("Source: {}", item.source)))
        .children(item.diagnostics.into_iter().map(|diagnostic| {
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
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
