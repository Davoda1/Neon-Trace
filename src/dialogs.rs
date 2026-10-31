//! Content-sized dialogs with a shared, readable type scale.
use super::*;
use iced::widget::{Column, column};

const TITLE_SIZE: u32 = 22;
const BODY_SIZE: u32 = 14;
const NOTE_SIZE: u32 = 12;

impl App {
    pub(super) fn modal_view(&self, modal: Modal) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let title = match modal {
            Modal::Targets => K::ConfigureTargets,
            Modal::Settings => K::Settings,
            Modal::About => K::About,
            Modal::Licenses => K::Licenses,
            Modal::History => K::History,
            Modal::Details => K::Details,
            Modal::Export => K::Export,
        };
        let mut heading = row![
            text(self.t(title))
                .size(TITLE_SIZE)
                .font(ui::bold())
                .width(Fill)
        ]
        .spacing(10)
        .align_y(iced::Center);
        if matches!(modal, Modal::Licenses | Modal::Settings) {
            heading = heading.push(self.dialog_button(self.t(K::About), Some(Message::About)));
        }
        heading = heading.push(self.dialog_button(self.t(K::Close), Some(Message::CloseModal)));

        // The outer panel has only a maximum height. Shrink-wrapped scroll
        // content grows naturally, and scrolls only once the viewport is full.
        // The title and target-editor actions remain outside that scroll area.
        let padding = if self.config.window_width < 960. {
            20.
        } else {
            24.
        };
        let footer_space = if modal == Modal::Targets { 52. } else { 0. };
        let available =
            (self.config.window_height - 32. - padding * 2. - 65. - footer_space).max(140.);
        let content = match modal {
            Modal::Targets => self.targets_dialog(),
            Modal::Settings => self.settings_dialog(),
            Modal::About => self.about_dialog(),
            Modal::Licenses => self.licenses_dialog(),
            Modal::Export => self.export_dialog(),
            Modal::History => self.history_dialog(),
            Modal::Details => container(self.inspector()).height(available).into(),
        };
        let body: Element<_> = if modal == Modal::Details {
            content
        } else {
            container(
                scrollable(container(content).padding(iced::Padding {
                    right: 12.,
                    ..Default::default()
                }))
                .height(Length::Shrink),
            )
            .max_height(available)
            .into()
        };
        let mut panel = column![heading, horizontal_rule(1), body].spacing(14);
        if modal == Modal::Targets {
            let dark = self.dark;
            panel = panel.push(
                row![
                    self.dialog_button(
                        self.t(K::AddTarget),
                        (self.fields.len() < 10).then_some(Message::AddTarget)
                    ),
                    Space::new().width(Fill),
                    self.dialog_button(self.t(K::Cancel), Some(Message::CloseModal)),
                    self.dialog_button(self.t(K::Apply), Some(Message::ApplyTargets))
                        .style(move |_, status| ui::primary(dark, status))
                ]
                .spacing(8)
                .align_y(iced::Center),
            );
        }
        container(panel)
            .style(move |_| container::Style {
                text_color: Some(p.text),
                ..Default::default()
            })
            .into()
    }

    fn dialog_button<'a>(
        &self,
        label: &'a str,
        message: Option<Message>,
    ) -> iced::widget::Button<'a, Message> {
        let dark = self.dark;
        button(text(label).size(BODY_SIZE))
            .on_press_maybe(message)
            .padding([8, 12])
            .style(move |_, status| ui::quiet(dark, status))
    }

    fn dialog_card<'a>(&self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        let dark = self.dark;
        container(content)
            .padding(14)
            .width(Fill)
            .style(move |_| ui::card(dark))
            .into()
    }

    fn dialog_section<'a>(
        &self,
        title: &'a str,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        self.dialog_card(column![text(title).size(BODY_SIZE).font(ui::bold()), content].spacing(10))
    }

    fn targets_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let dark = self.dark;
        let mut entries = column![].spacing(8);
        for (index, value) in self.fields.iter().enumerate() {
            let invalid = self.field_errors.get(index).and_then(|value| *value);
            let mut item = column![
                row![
                    text(format!("{:02}", index + 1))
                        .size(NOTE_SIZE)
                        .color(p.muted)
                        .width(22),
                    text_input(self.t(K::TargetHint), value)
                        .on_input(move |value| Message::TargetInput(index, value))
                        .padding([9, 11])
                        .size(BODY_SIZE)
                        .width(Fill)
                        .style(move |theme, state| {
                            let mut style = iced::widget::text_input::default(theme, state);
                            style.background = p.bg.into();
                            style.value = p.text;
                            style.placeholder = p.muted;
                            style.border = iced::Border {
                                color: if invalid.is_some() {
                                    Color::from_rgb8(211, 93, 99)
                                } else if matches!(
                                    state,
                                    iced::widget::text_input::Status::Focused { .. }
                                ) {
                                    p.blue
                                } else {
                                    p.line
                                },
                                width: 1.,
                                radius: 8.0.into(),
                            };
                            style
                        }),
                    self.dialog_button(
                        self.t(K::Remove),
                        (self.fields.len() > 1).then_some(Message::RemoveTarget(index))
                    )
                    .style(move |_, status| ui::quiet(dark, status))
                ]
                .spacing(9)
                .align_y(iced::Center)
            ]
            .spacing(5);
            if let Some(key) = invalid {
                item = item.push(
                    container(text(self.t(key)).size(NOTE_SIZE).color(if dark {
                        Color::from_rgb8(255, 155, 159)
                    } else {
                        Color::from_rgb8(161, 43, 56)
                    }))
                    .padding(iced::Padding {
                        left: 31.,
                        ..Default::default()
                    }),
                );
            }
            entries = entries.push(item);
        }
        column![
            text(self.t(K::TargetListHint))
                .size(BODY_SIZE)
                .color(p.muted),
            entries
        ]
        .spacing(16)
        .into()
    }

    fn settings_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let appearance = column![
            row![
                text(self.t(K::Language)).size(BODY_SIZE).width(Fill),
                pick_list(Language::ALL, Some(self.language), Message::Language)
                    .text_size(BODY_SIZE)
                    .padding([7, 10])
            ]
            .spacing(10)
            .align_y(iced::Center),
            text(self.t(K::SystemTheme)).size(NOTE_SIZE).color(p.muted),
            checkbox(self.config.reduced_effects)
                .label(self.t(K::ReducedEffects))
                .text_size(BODY_SIZE)
                .size(17)
                .on_toggle(Message::Effects)
        ]
        .spacing(12);
        let measurement = column![
            checkbox(self.config.tcp_check)
                .label(self.t(K::TcpCheck))
                .text_size(BODY_SIZE)
                .size(17)
                .on_toggle(Message::Tcp),
            text(self.t(K::TcpNote)).size(NOTE_SIZE).color(p.muted)
        ]
        .spacing(8);
        let history = column![
            checkbox(self.config.history_enabled)
                .label(self.t(K::EnableHistory))
                .text_size(BODY_SIZE)
                .size(17)
                .on_toggle(Message::HistoryEnabled),
            text(self.t(if self.config.history_enabled {
                K::HistoryRetention
            } else {
                K::HistoryDisabled
            }))
            .size(NOTE_SIZE)
            .color(p.muted),
            self.dialog_button(self.t(K::History), Some(Message::History))
        ]
        .spacing(10);
        let mut databases = column![text(self.t(self.geo_status)).size(BODY_SIZE)].spacing(9);
        if let Some((bytes, total)) = self.geo_bytes {
            databases = databases.push(
                text(format!(
                    "{:.1} MB / {}",
                    bytes as f64 / 1e6,
                    total
                        .map(|value| format!("{:.1} MB", value as f64 / 1e6))
                        .unwrap_or_else(|| "—".into())
                ))
                .size(NOTE_SIZE)
                .color(p.muted),
            );
        }
        if let Some(error) = &self.geo_error {
            databases = databases.push(text(error).size(13).color(if self.dark {
                Color::from_rgb8(255, 155, 159)
            } else {
                Color::from_rgb8(161, 43, 56)
            }));
        }
        if let Some(service) = &self.geo {
            for edition in service.editions() {
                databases = databases.push(
                    text(format!(
                        "DB-IP {} · {} · {}",
                        self.t(if edition.kind == geo::DatasetKind::City {
                            K::CityDatabase
                        } else {
                            K::AsnDatabase
                        }),
                        edition.edition,
                        chrono::DateTime::from_timestamp(edition.installed_unix as i64, 0)
                            .map(|date| date.format("%Y-%m-%d").to_string())
                            .unwrap_or_default()
                    ))
                    .size(NOTE_SIZE)
                    .color(p.muted),
                );
            }
        }
        databases = databases.push(self.dialog_button(
            self.t(K::Refresh),
            (!self.geo_busy && !self.demonstration).then_some(Message::Refresh),
        ));
        column![
            text(self.t(K::SettingsHint)).size(BODY_SIZE).color(p.muted),
            self.dialog_section(self.t(K::Appearance), appearance.into()),
            self.dialog_section(self.t(K::Measurements), measurement.into()),
            self.dialog_section(self.t(K::History), history.into()),
            self.dialog_section(self.t(K::LocationData), databases.into()),
            self.dialog_section(
                self.t(K::Privacy),
                text(self.t(K::PrivacyNote))
                    .size(NOTE_SIZE)
                    .color(p.muted)
                    .into()
            )
        ]
        .spacing(12)
        .into()
    }

    fn export_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let dark = self.dark;
        let enabled = !self.sessions.is_empty();
        let mut formats =
            column![text(self.t(K::ExportHint)).size(BODY_SIZE).color(p.muted)].spacing(12);
        if !enabled {
            formats =
                formats.push(self.dialog_card(text(self.t(K::NothingToExport)).size(BODY_SIZE)));
        }
        for (format, title, hint) in [
            ("png", K::ExportPng, K::ExportPngHint),
            ("json", K::ExportJson, K::ExportJsonHint),
            ("csv", K::ExportCsv, K::ExportCsvHint),
        ] {
            formats = formats.push(
                button(
                    row![
                        container(
                            text(format.to_uppercase())
                                .size(NOTE_SIZE)
                                .font(ui::bold())
                                .color(p.blue)
                        )
                        .width(55)
                        .center_y(42),
                        column![
                            text(self.t(title))
                                .size(BODY_SIZE)
                                .font(ui::bold())
                                .color(if enabled { p.text } else { p.muted }),
                            text(self.t(hint)).size(NOTE_SIZE).color(p.muted)
                        ]
                        .spacing(5)
                        .width(Fill),
                        canvas(ui::ActionIcon {
                            kind: ui::IconKind::Export,
                            color: p.muted
                        })
                        .width(17)
                        .height(17)
                    ]
                    .spacing(12)
                    .align_y(iced::Center),
                )
                .on_press_maybe(enabled.then_some(Message::Export(format)))
                .padding(14)
                .width(Fill)
                .style(move |_, status| ui::quiet(dark, status)),
            );
        }
        formats
            .push(horizontal_rule(1))
            .push(
                text(self.t(K::ExportPrivacy))
                    .size(NOTE_SIZE)
                    .color(p.muted),
            )
            .into()
    }

    fn about_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        column![
            row![
                canvas(ui::BrandMark { dark: self.dark })
                    .width(48)
                    .height(48),
                column![
                    text("NeonTrace").size(22).font(ui::bold()),
                    text(env!("CARGO_PKG_VERSION"))
                        .size(NOTE_SIZE)
                        .color(p.muted)
                ]
                .spacing(5)
            ]
            .spacing(14)
            .align_y(iced::Center),
            column![
                text(self.t(K::AboutIntro)).size(BODY_SIZE).font(ui::bold()),
                text(self.t(K::AboutDescription))
                    .size(BODY_SIZE)
                    .color(p.muted)
            ]
            .spacing(8),
            self.dialog_section(
                self.t(K::Credits),
                column![
                    text("Copyright © 2026 Davoda1 · MIT").size(BODY_SIZE),
                    text(self.t(K::MapCredit)).size(NOTE_SIZE).color(p.muted),
                    text(self.t(K::GeoCredit)).size(NOTE_SIZE).color(p.muted),
                    self.dialog_button(self.t(K::Licenses), Some(Message::Licenses))
                ]
                .spacing(10)
                .into()
            ),
            self.dialog_section(
                self.t(K::Privacy),
                text(self.t(K::PrivacyNote))
                    .size(NOTE_SIZE)
                    .color(p.muted)
                    .into()
            )
        ]
        .spacing(18)
        .into()
    }

    fn licenses_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let mut sections = Column::new().spacing(14);
        for (title, notice) in [
            ("NeonTrace · MIT", include_str!("../LICENSE")),
            ("Simplemaps · MIT", map::MAP_NOTICE),
            ("DejaVu Sans", include_str!("../assets/DEJAVU-NOTICE.txt")),
        ] {
            sections = sections.push(self.dialog_section(
                title,
                text(notice.trim()).size(NOTE_SIZE).color(p.muted).into(),
            ));
        }
        sections
            .push(
                self.dialog_section(
                    self.t(K::DataSource),
                    column![
                        text(map::SOURCE_URL).size(NOTE_SIZE).color(p.muted),
                        text(geo::DATA_CREDIT).size(NOTE_SIZE).color(p.muted)
                    ]
                    .spacing(10)
                    .into(),
                ),
            )
            .into()
    }

    fn history_dialog(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let mut entries = column![self.dialog_button(
            self.t(K::ClearHistory),
            (!self.history_entries.is_empty()).then_some(Message::ClearHistory)
        )]
        .spacing(12);
        if self.history_entries.is_empty() {
            entries = entries.push(
                self.dialog_card(text(self.t(K::EmptyHistory)).size(BODY_SIZE).color(p.muted)),
            );
        }
        for item in &self.history_entries {
            let saved = chrono::DateTime::from_timestamp_millis(item.saved_unix_ms as i64)
                .map(|date| date.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default();
            entries = entries.push(
                self.dialog_card(
                    column![
                        text(item.targets.join(", "))
                            .size(BODY_SIZE)
                            .font(ui::bold()),
                        text(format!(
                            "{} · {} · {:.1} KB",
                            saved,
                            self.t(if item.demonstration {
                                K::DemoLabel
                            } else {
                                K::SavedResult
                            }),
                            item.size_bytes as f64 / 1000.
                        ))
                        .size(NOTE_SIZE)
                        .color(p.muted),
                        row![
                            self.dialog_button(
                                self.t(K::Replay),
                                Some(Message::LoadHistory(item.id.clone()))
                            ),
                            self.dialog_button(
                                self.t(K::DeleteRecord),
                                Some(Message::DeleteHistory(item.id.clone()))
                            )
                        ]
                        .spacing(8)
                    ]
                    .spacing(10),
                ),
            );
        }
        entries.into()
    }
}
