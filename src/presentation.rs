//! Native presentation of measured observations and independently timed journeys.
use super::*;
use iced::widget::column;

impl App {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let dark = self.dark;
        let p = ui::palette(dark);
        let narrow = self.config.window_width < 960.0;
        let small = self.config.window_height < 650.0;
        let brand = row![
            canvas(ui::BrandMark { dark }).width(38).height(38),
            column![
                row![
                    text("Neon").size(24).font(ui::bold()),
                    text("Trace").size(24).color(p.muted)
                ]
                .spacing(0),
                text(self.t(K::AppSubtitle)).size(10).color(p.muted)
            ]
            .spacing(2)
        ]
        .spacing(11)
        .align_y(iced::Center);
        let mut utilities = row![].spacing(7).align_y(iced::Center);
        if !narrow {
            utilities = utilities
                .push(self.quiet_action(self.t(K::History), Message::History))
                .push(self.quiet_action(self.t(K::About), Message::About));
        }
        utilities = utilities
            .push(self.quiet_action(self.t(K::Export), Message::OpenExport))
            .push(self.quiet_action(self.t(K::Settings), Message::Settings));
        let header = row![brand, Space::new().width(Fill), utilities]
            .spacing(12)
            .align_y(iced::Center)
            .height(if small { 43 } else { 56 });

        let destination: Element<_> = if self.multi {
            button(
                row![
                    text(self.t(K::Targets)).font(ui::bold()),
                    text(format!("{:02}", self.multi_targets.len())).color(p.green),
                    text(self.t(K::ConfigureTargets)).size(12).color(p.muted)
                ]
                .spacing(12)
                .align_y(iced::Center),
            )
            .padding([11, 14])
            .width(Fill)
            .on_press(Message::OpenTargets)
            .style(move |_, status| ui::quiet(dark, status))
            .into()
        } else {
            text_input(self.t(K::TargetHint), &self.input)
                .on_input(Message::Input)
                .on_submit(Message::Start)
                .padding([11, 14])
                .size(14)
                .width(Fill)
                .style(move |theme, status| {
                    let mut style = iced::widget::text_input::default(theme, status);
                    style.background = p.panel.into();
                    style.border = iced::Border {
                        color: p.line,
                        width: 1.0,
                        radius: 10.0.into(),
                    };
                    style.value = p.text;
                    style.placeholder = p.muted;
                    if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
                        style.border.color = p.blue;
                    }
                    style
                })
                .into()
        };
        let command = row![
            destination,
            button(
                row![
                    canvas(ui::ActionIcon {
                        kind: if self.running {
                            ui::IconKind::Stop
                        } else {
                            ui::IconKind::Play
                        },
                        color: if self.running {
                            if dark {
                                Color::from_rgb8(255, 184, 190)
                            } else {
                                Color::from_rgb8(148, 47, 60)
                            }
                        } else {
                            Color::from_rgb8(12, 36, 24)
                        },
                    })
                    .width(15)
                    .height(15),
                    text(self.t(if self.running { K::Stop } else { K::Start }))
                        .font(ui::bold())
                        .size(14)
                ]
                .spacing(8)
                .align_y(iced::Center)
            )
            .on_press(if self.running {
                Message::Stop
            } else {
                Message::Start
            })
            .padding([11, 18])
            .style(move |_, status| if self.running {
                ui::stop(dark, status)
            } else {
                ui::primary(dark, status)
            }),
            button(self.action_label(
                self.t(if self.multi {
                    K::SingleTarget
                } else if narrow {
                    K::Targets
                } else {
                    K::MultiTarget
                }),
                ui::IconKind::Targets,
                12.,
                if self.multi { p.green } else { p.muted },
            ))
            .on_press(Message::ToggleMulti)
            .padding([11, 13])
            .style(move |_, status| if self.multi {
                ui::selected(dark, status)
            } else {
                ui::quiet(dark, status)
            })
        ]
        .spacing(9)
        .align_y(iced::Center);

        let map: Element<map_view::MapEvent> = canvas(MapView {
            cache: &self.cache,
            routes: &self.routes,
            selected_id: self.sessions.get(self.selected).map(|s| s.target_id),
            selected_node: self.selected_map_node,
            reset_epoch: self.map_reset_epoch,
            dark,
        })
        .width(Fill)
        .height(Fill)
        .into();
        let world_title = column![
            text(self.t(K::WorldConnected))
                .size(11)
                .font(ui::bold())
                .color(p.text),
            text(if self.multi {
                self.t(K::MultiViewHint).to_owned()
            } else {
                self.route_caption()
            })
            .size(12)
            .color(p.muted)
            .width(Fill)
        ]
        .spacing(5)
        .width(Fill);
        let mut map_actions = row![self.quiet_action(self.t(K::FitMap), Message::FitMap)]
            .spacing(6)
            .align_y(iced::Center);
        if !narrow && !self.multi {
            map_actions = map_actions.push(self.quiet_action(
                self.t(if self.focus_map {
                    K::ShowPanel
                } else {
                    K::MapFocus
                }),
                Message::FocusMap,
            ));
        } else {
            map_actions = map_actions
                .push(self.quiet_action(self.t(K::Details), Message::Select(self.selected)));
        }
        let map_header = row![world_title, map_actions]
            .spacing(10)
            .align_y(iced::Center);
        let mut playback = row![
            button(self.action_label(
                self.t(if self.presentation_paused {
                    K::Resume
                } else {
                    K::Pause
                }),
                if self.presentation_paused {
                    ui::IconKind::Play
                } else {
                    ui::IconKind::Pause
                },
                11.,
                p.muted,
            ))
            .on_press_maybe((!self.routes.is_empty()).then_some(Message::Pause))
            .padding([7, 10])
            .style(move |_, status| ui::quiet(dark, status)),
            button(self.action_label(self.t(K::Replay), ui::IconKind::Replay, 11., p.green))
                .on_press_maybe((!self.routes.is_empty()).then_some(Message::Replay))
                .padding([7, 11])
                .style(move |_, status| ui::selected(dark, status))
        ]
        .spacing(6)
        .align_y(iced::Center);
        if !self.running && (!self.sessions.is_empty() || !self.input.is_empty() || self.multi) {
            playback = playback.push(
                button(text(self.t(K::ClearView)).size(12))
                    .on_press(Message::Clear)
                    .padding([7, 11])
                    .style(move |_, status| ui::quiet(dark, status)),
            );
        }
        let legend: Element<_> = if narrow {
            text(self.t(K::DisplayedJourney))
                .size(10)
                .color(p.muted)
                .into()
        } else {
            row![
                text("—").color(p.green),
                text(self.t(if self.demonstration {
                    K::ExampleRoute
                } else {
                    K::LocatedRoute
                }))
                .size(10)
                .color(p.muted),
                text("┄").color(Color::from_rgb8(204, 173, 112)),
                text(self.t(K::UnknownSection)).size(10).color(p.muted)
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into()
        };
        let playback_row: Element<_> = if narrow {
            column![legend, row![Space::new().width(Fill), playback]]
                .spacing(4)
                .into()
        } else {
            row![legend, Space::new().width(Fill), playback]
                .spacing(8)
                .align_y(iced::Center)
                .into()
        };
        let map_footer = column![
            text(self.t(K::MapNavigationHint)).size(11).color(p.muted),
            playback_row
        ]
        .spacing(5);
        let mut map_body = column![map_header, map.map(Message::MapSelected), map_footer]
            .spacing(8)
            .height(Fill);
        if self.geo_busy && !self.demonstration {
            let amount = self
                .geo_bytes
                .map(|(received, total)| {
                    format!(
                        "{:.1} MB / {}",
                        received as f64 / 1e6,
                        total
                            .map(|n| format!("{:.1} MB", n as f64 / 1e6))
                            .unwrap_or_else(|| "—".into())
                    )
                })
                .unwrap_or_default();
            map_body = map_body.push(
                row![
                    text(self.t(self.geo_status)).size(11).width(Fill),
                    text(amount).size(11)
                ]
                .spacing(8),
            );
            if let Some((received, Some(total))) = self
                .geo_bytes
                .filter(|(_, total)| total.is_some_and(|t| t > 0))
            {
                map_body = map_body.push(
                    progress_bar(
                        0.0..=1.0,
                        (received as f64 / total as f64).clamp(0.0, 1.0) as f32,
                    )
                    .girth(3),
                );
            }
        }
        let map_panel = container(map_body)
            .padding(if small { 12 } else { 18 })
            .width(Fill)
            .height(Fill)
            .style(move |_| {
                let mut s = ui::panel(dark);
                s.background = Some(
                    if dark {
                        Color::from_rgb8(8, 15, 25)
                    } else {
                        Color::from_rgb8(215, 226, 235)
                    }
                    .into(),
                );
                s
            });
        let center: Element<_> = if !narrow && !self.focus_map && !self.multi {
            row![
                map_panel,
                container(self.inspector())
                    .padding(18)
                    .width(284)
                    .height(Fill)
                    .style(move |_| ui::panel(dark))
            ]
            .spacing(14)
            .height(Fill)
            .into()
        } else {
            map_panel.into()
        };
        let mut body = column![header, horizontal_rule(1), command, center]
            .spacing(if small { 9 } else { 13 })
            .height(Fill);
        body = if self.multi {
            body.push(self.target_strip(narrow))
        } else {
            body.push(self.hop_strip(narrow))
        };
        let footer_status = if self.demonstration {
            self.t(K::DemoWarning)
        } else {
            self.t(self.status)
        };
        let mut footer = row![
            button(text(self.t(K::DemoShort)).size(10))
                .on_press(Message::Demo)
                .padding([5, 8])
                .style(move |_, status| if self.demonstration {
                    ui::selected(dark, status)
                } else {
                    ui::quiet(dark, status)
                }),
            text(footer_status).size(10).color(p.muted).width(Fill)
        ]
        .spacing(10)
        .align_y(iced::Center);
        if !narrow {
            footer = footer.push(
                text("Simplemaps · IP Geolocation by DB-IP")
                    .size(9)
                    .color(p.muted),
            );
        }
        body = body.push(footer);
        let base: Element<_> = container(body)
            .padding(if narrow { 14 } else { 26 })
            .height(Fill)
            .into();
        if let Some(modal) = self.modal {
            stack![
                base,
                opaque(
                    container(
                        container(iced::widget::keyed_column([(
                            modal as u8,
                            self.modal_view(modal),
                        )]))
                        .padding(if narrow { 20 } else { 24 })
                        .width(Fill)
                        .max_width(700)
                        .max_height((self.config.window_height - 32.0).max(300.0))
                        .style(move |_| ui::panel(dark))
                    )
                    .center_x(Fill)
                    .center_y(Fill)
                    .width(Fill)
                    .height(Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba8(2, 6, 12, 0.78).into()),
                        ..Default::default()
                    })
                )
            ]
            .into()
        } else {
            // Preserve the base widget tree (including manual map navigation)
            // when a dialog is opened or closed.
            stack![base].into()
        }
    }

    fn action_label<'a>(
        &self,
        label: &'a str,
        icon: ui::IconKind,
        size: f32,
        color: Color,
    ) -> Element<'a, Message> {
        row![
            canvas(ui::ActionIcon { kind: icon, color })
                .width(14)
                .height(14),
            text(label).size(size)
        ]
        .spacing(7)
        .align_y(iced::Center)
        .into()
    }

    fn quiet_action<'a>(
        &self,
        label: &'a str,
        message: Message,
    ) -> iced::widget::Button<'a, Message> {
        let dark = self.dark;
        let icon = match message {
            Message::History => ui::IconKind::History,
            Message::About => ui::IconKind::Info,
            Message::OpenExport => ui::IconKind::Export,
            Message::Settings => ui::IconKind::Settings,
            Message::FocusMap | Message::FitMap => ui::IconKind::Focus,
            _ => ui::IconKind::Details,
        };
        button(self.action_label(label, icon, 12., ui::palette(dark).muted))
            .on_press(message)
            .padding([8, 11])
            .style(move |_, status| ui::quiet(dark, status))
    }

    fn route_caption(&self) -> String {
        let Some(s) = self.sessions.get(self.selected) else {
            return self.t(K::EnterDestination).into();
        };
        let origin = s
            .origin
            .as_ref()
            .and_then(|p| p.city.as_deref())
            .unwrap_or(self.t(K::YourPc));
        let endpoint = s
            .endpoint
            .as_ref()
            .and_then(|p| p.city.as_deref())
            .unwrap_or(&s.target);
        format!("{origin}  →  {endpoint}")
    }

    pub(super) fn inspector(&self) -> Element<'_, Message> {
        let dark = self.dark;
        let tabs = row![
            button(
                text(self.t(K::Overview))
                    .size(12)
                    .width(Fill)
                    .align_x(iced::Center)
            )
            .width(Fill)
            .padding([8, 8])
            .on_press(Message::InspectorTab(false))
            .style(move |_, status| if self.show_hops {
                ui::quiet(dark, status)
            } else {
                ui::selected(dark, status)
            }),
            button(
                text(self.t(K::Hops))
                    .size(12)
                    .width(Fill)
                    .align_x(iced::Center)
            )
            .width(Fill)
            .padding([8, 8])
            .on_press(Message::InspectorTab(true))
            .style(move |_, status| if self.show_hops {
                ui::selected(dark, status)
            } else {
                ui::quiet(dark, status)
            })
        ]
        .spacing(5);
        let content = if self.show_hops {
            self.hop_details()
        } else {
            self.overview()
        };
        column![
            tabs,
            scrollable(container(content).padding(iced::Padding {
                right: 16.0,
                ..Default::default()
            }))
            .height(Fill)
        ]
        .spacing(18)
        .height(Fill)
        .into()
    }

    fn overview(&self) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let dark = self.dark;
        let Some(s) = self.sessions.get(self.selected) else {
            return column![
                text(self.t(K::Endpoint)).size(21).font(ui::bold()),
                text(self.t(K::EnterDestination)).size(14),
                horizontal_rule(1),
                text(self.t(K::DownloadOnFirstUse)).size(12).color(p.muted),
                text(self.t(K::PrivacyNote)).size(11).color(p.muted)
            ]
            .spacing(17)
            .into();
        };
        let stats = s.statistics();
        let identity = column![
            text(self.t(K::Endpoint)).size(10).color(p.muted),
            text(&s.target)
                .size(17)
                .font(ui::bold())
                .wrapping(iced::widget::text::Wrapping::Glyph),
            button(text(s.chosen_ip.as_deref().unwrap_or("—")).size(11))
                .on_press(Message::Copy(s.chosen_ip.clone().unwrap_or_default()))
                .padding(0)
                .style(button_style::text),
            row![
                text("●").size(8).color(if s.demonstration {
                    p.blue
                } else if !self.running {
                    p.muted
                } else if s.errors.iter().any(|e| e.code != "demonstration") {
                    Color::from_rgb8(211, 163, 83)
                } else {
                    p.green
                }),
                text(self.t(self.session_status(s))).size(11).color(p.muted),
                Space::new().width(Fill),
                text(s.address_family.as_deref().unwrap_or("—"))
                    .size(10)
                    .color(p.muted)
            ]
            .spacing(6)
            .align_y(iced::Center)
        ]
        .spacing(5);
        let last = s
            .pings
            .last()
            .and_then(|v| v.rtt_ms)
            .filter(|v| v.is_finite());
        let metric = row![
            text(
                last.map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "—".into())
            )
            .size(42),
            text("ms").size(15).color(p.muted)
        ]
        .spacing(5)
        .align_y(iced::Bottom);
        let mini = |key: K, value: Option<f64>| {
            column![
                text(self.t(key)).size(10).color(p.muted),
                text(
                    value
                        .map(|v| format!("{v:.1}"))
                        .unwrap_or_else(|| "—".into())
                )
                .size(13)
            ]
            .spacing(4)
            .width(Fill)
        };
        let samples = s.pings.iter().map(|v| v.rtt_ms).collect();
        let mut c = column![
            identity,
            horizontal_rule(1),
            text(self.t(K::CurrentRtt)).size(11).color(p.muted),
            metric,
            canvas(ui::Sparkline { samples, dark })
                .height(48)
                .width(Fill),
            row![
                mini(K::Minimum, stats.minimum_ms),
                mini(K::Average, stats.average_ms),
                mini(K::Maximum, stats.maximum_ms)
            ]
            .spacing(8),
            row![
                text(format!(
                    "{} / {} {}",
                    stats.replies,
                    stats.sent,
                    self.t(K::Replies)
                ))
                .size(10)
                .color(p.muted),
                Space::new().width(Fill),
                text(format!(
                    "{} {}",
                    stats.nonresponses,
                    self.t(K::Nonresponses)
                ))
                .size(10)
                .color(p.muted)
            ]
            .spacing(8),
            horizontal_rule(1)
        ]
        .spacing(10);
        let trace_status = if s.destination_reached {
            K::Complete
        } else if s.trace_complete {
            K::PartialRoute
        } else {
            K::NoRoute
        };
        c = c.push(
            row![
                text(self.t(K::RouteSnapshot)).size(11).color(p.muted),
                Space::new().width(Fill),
                text(
                    s.route_timestamp_unix_ms
                        .map(|t| self.seconds(export::now_ms().saturating_sub(t) / 1000))
                        .unwrap_or_else(|| "—".into())
                )
                .size(11)
                .color(p.muted)
            ]
            .spacing(8),
        );
        c = c.push(
            row![
                text(self.t(trace_status)).size(12).width(Fill),
                button(text(self.t(K::TraceAgain)).size(10))
                    .padding([6, 8])
                    .on_press_maybe(
                        (self.running && !self.demonstration && !self.saved)
                            .then_some(Message::Retrace)
                    )
                    .style(move |_, status| ui::quiet(dark, status))
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
        c = c.push(horizontal_rule(1)).push(
            text(self.t(if s.demonstration {
                K::Origin
            } else {
                K::FromThisComputer
            }))
            .size(10)
            .color(p.muted),
        );
        c = c.push(
            text(
                s.origin
                    .as_ref()
                    .map(|v| location(v, self.t(K::LocationUnknown)))
                    .unwrap_or_else(|| self.t(K::OriginUnavailable).into()),
            )
            .size(13),
        );
        c = c.push(text(self.t(K::EstimatedOrigin)).size(10).color(p.muted));
        c = c.push(
            button(
                text(self.t(if self.expanded_details {
                    K::HideDetails
                } else {
                    K::MoreDetails
                }))
                .size(11),
            )
            .padding([6, 0])
            .on_press(Message::DetailToggle)
            .style(button_style::text),
        );
        if self.expanded_details {
            c = c.push(text(self.t(K::SampleWindow)).size(10).color(p.muted));
            if let Some(pnt) = &s.endpoint {
                c = c.push(text(location(pnt, self.t(K::LocationUnknown))).size(12));
                if let Some(asn) = pnt.asn {
                    c = c.push(
                        text(format!(
                            "AS{asn} · {}",
                            pnt.organization.as_deref().unwrap_or(self.t(K::Unknown))
                        ))
                        .size(11),
                    );
                }
                if let Some(provider) = &pnt.provider {
                    c = c.push(
                        text(format!("{}: {provider}", self.t(K::DataSource)))
                            .size(10)
                            .color(p.muted),
                    );
                }
                c = c.push(
                    text(format!(
                        "{}: {} / {}",
                        self.t(K::DataEdition),
                        pnt.city_edition.as_deref().unwrap_or("—"),
                        pnt.asn_edition.as_deref().unwrap_or("—")
                    ))
                    .size(10)
                    .color(p.muted),
                );
            }
            if let Some(tcp) = s.tcp_checks.last() {
                c = c.push(
                    text(format!(
                        "TCP 443 · {} · {}",
                        self.t(if tcp.status == SampleStatus::Reply {
                            K::Complete
                        } else {
                            K::Unavailable
                        }),
                        ms(tcp.connection_ms)
                    ))
                    .size(11),
                );
            }
            for e in s.errors.iter().rev().take(3) {
                c = c.push(
                    text(format!("{}: {}", e.code, e.detail))
                        .size(10)
                        .color(p.muted),
                );
            }
        }
        c.into()
    }

    fn hop_details(&self) -> Element<'_, Message> {
        let dark = self.dark;
        let p = ui::palette(dark);
        let Some(s) = self.sessions.get(self.selected) else {
            return text(self.t(K::NoHops)).size(13).into();
        };
        let route = self.routes.iter().find(|r| r.id == s.target_id);
        let selected_node = self
            .selected_map_node
            .and_then(|(id, n)| route.filter(|r| r.id == id).and_then(|r| r.nodes.get(n)));
        let mut c = column![
            text(self.t(K::AllHops)).size(16).font(ui::bold()),
            text(self.t(K::SelectLocation)).size(11).color(p.muted)
        ]
        .spacing(10);
        if let Some(node) = selected_node {
            c = c.push(
                container(
                    column![
                        text(self.t(K::SelectedHop)).size(10).color(p.muted),
                        text(&node.label).size(17).font(ui::bold()),
                        text(format!("{:.2}°, {:.2}°", node.lat, node.lon))
                            .size(10)
                            .color(p.muted)
                    ]
                    .spacing(5),
                )
                .padding(12)
                .width(Fill)
                .style(move |_| ui::card(dark)),
            );
        }
        if s.hops.is_empty() {
            c = c.push(text(self.t(K::NoHops)).size(12));
        }
        for h in &s.hops {
            let mut items = column![
                row![
                    text(format!("{:02}", h.ttl)).size(12).color(p.blue),
                    text(self.t(if h.responses.is_empty() {
                        K::NoReply
                    } else {
                        K::Hop
                    }))
                    .size(11)
                    .color(p.muted)
                ]
                .spacing(8)
            ]
            .spacing(6);
            for response in &h.responses {
                let node = response.location.as_ref().and_then(|geo| {
                    route.and_then(|r| {
                        r.nodes.iter().position(|n| {
                            geo.latitude.zip(geo.longitude).is_some_and(|(lat, lon)| {
                                (lat - n.lat).abs() < 0.001 && (lon - n.lon).abs() < 0.001
                            })
                        })
                    })
                });
                let selected =
                    node.is_some_and(|n| self.selected_map_node == Some((s.target_id, n)));
                let location_name = response
                    .location
                    .as_ref()
                    .map(|v| location(v, self.t(K::LocationUnknown)))
                    .unwrap_or_else(|| self.t(K::LocationUnknown).into());
                let mut details = column![
                    text(&response.address)
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::Glyph),
                    row![
                        text(location_name).size(11).width(Fill),
                        text(ms(response.rtt_ms)).size(11)
                    ]
                    .spacing(6)
                ]
                .spacing(5);
                if let Some(geo) = &response.location {
                    if let Some(asn) = geo.asn {
                        details = details.push(
                            text(format!(
                                "AS{asn} · {}",
                                geo.organization.as_deref().unwrap_or(self.t(K::Unknown))
                            ))
                            .size(10)
                            .color(p.muted),
                        );
                    }
                }
                items = items.push(
                    button(details)
                        .padding(9)
                        .width(Fill)
                        .on_press(
                            node.map(|n| {
                                Message::MapSelected(map_view::MapEvent::Selected {
                                    route_id: s.target_id,
                                    node_index: n,
                                })
                            })
                            .unwrap_or_else(|| Message::Copy(response.address.clone())),
                        )
                        .style(move |_, status| {
                            if selected {
                                ui::selected(dark, status)
                            } else {
                                ui::quiet(dark, status)
                            }
                        }),
                );
            }
            c = c.push(items).push(horizontal_rule(1));
        }
        c.into()
    }

    pub(super) fn follow_journey(&mut self, now: Instant) -> Task<Message> {
        let narrow = self.config.window_width < 960.0;
        let Some(route) = self.routes.get(self.selected).filter(|route| {
            !self.multi && route.nodes.len().saturating_sub(1) > if narrow { 3 } else { 6 }
        }) else {
            self.journey_follow = journey::Follow::default();
            return Task::none();
        };
        self.journey_follow.route((
            route.id,
            self.route_ids.get(&route.id).copied().unwrap_or_default(),
        ));
        let Some(active) = journey::active_segment(&route.segment_progresses()) else {
            return Task::none();
        };
        self.journey_follow
            .advance(
                now,
                active,
                !self.presentation_paused && self.modal.is_none(),
                !self.config.reduced_effects,
            )
            .map(|x| {
                iced::widget::operation::scroll_to(
                    journey::SCROLL_ID,
                    iced::widget::operation::AbsoluteOffset { x, y: 0.0 },
                )
            })
            .unwrap_or_else(Task::none)
    }

    fn hop_strip(&self, narrow: bool) -> Element<'_, Message> {
        let p = ui::palette(self.dark);
        let dark = self.dark;
        let Some(route) = self.routes.get(self.selected).filter(|r| r.nodes.len() > 1) else {
            return container(
                text(self.t(if self.sessions.is_empty() {
                    K::EnterDestination
                } else {
                    K::LocationUnknown
                }))
                .size(12)
                .color(p.muted),
            )
            .padding([10, 0])
            .into();
        };
        let count = route.nodes.len() - 1;
        let mut bars = row![].spacing(journey::CARD_GAP);
        let fractions = route.segment_progresses();
        for (i, fraction) in fractions.into_iter().enumerate() {
            let node = &route.nodes[i + 1];
            let name = row![
                text(format!("{:02}", i + 1))
                    .size(10)
                    .color(if fraction > 0.0 { p.green } else { p.muted }),
                text(&node.label).size(12).color(p.text)
            ]
            .spacing(8);
            let description = self.t(if node.unknown_before {
                K::LocationUnknown
            } else if i + 1 == count {
                K::Endpoint
            } else {
                K::Hop
            });
            let content = column![
                progress_bar(0.0..=1.0, fraction).girth(3).style(move |_| {
                    iced::widget::progress_bar::Style {
                        background: p.line.into(),
                        bar: p.green.into(),
                        border: iced::Border::default(),
                    }
                }),
                name,
                text(description).size(10).color(p.muted)
            ]
            .spacing(7);
            bars = bars.push(
                button(content)
                    .on_press(Message::MapSelected(map_view::MapEvent::Selected {
                        route_id: route.id,
                        node_index: i + 1,
                    }))
                    .padding([5, 0])
                    .style(move |_, status| {
                        let mut s = ui::quiet(dark, status);
                        s.background = None;
                        s.border = iced::Border::default();
                        s.shadow = iced::Shadow::default();
                        s
                    })
                    .width(if count > if narrow { 3 } else { 6 } {
                        Length::Fixed(journey::CARD_WIDTH)
                    } else {
                        Fill
                    }),
            );
        }
        let transitions: Element<_> = if count > if narrow { 3 } else { 6 } {
            iced::widget::mouse_area(
                scrollable(bars)
                    .id(journey::SCROLL_ID)
                    .direction(iced::widget::scrollable::Direction::Horizontal(
                        iced::widget::scrollable::Scrollbar::default(),
                    ))
                    // Embed the rail below the naturally sized cards. Wrapped place
                    // names must not be clipped by a fixed height or floating rail.
                    .spacing(6)
                    .on_scroll(Message::JourneyViewport),
            )
            .on_enter(Message::JourneyHover(true))
            .on_exit(Message::JourneyHover(false))
            .into()
        } else {
            bars.width(Fill).into()
        };
        let label: Element<_> = if narrow {
            Space::new().width(0).into()
        } else {
            column![
                text(self.t(K::Journey)).size(10).color(p.muted),
                text(self.t(K::DisplayedJourney)).size(11)
            ]
            .spacing(5)
            .width(132)
            .into()
        };
        row![label, transitions]
            .spacing(if narrow { 0 } else { 22 })
            .align_y(iced::Center)
            .into()
    }

    fn target_strip(&self, narrow: bool) -> Element<'_, Message> {
        let dark = self.dark;
        let p = ui::palette(dark);
        if self.sessions.is_empty() {
            return text(self.t(K::Idle)).size(12).color(p.muted).into();
        }
        let available = (self.config.window_width - if narrow { 28. } else { 52. }).max(1.);
        let card_width = 198.;
        let count = self.sessions.len();
        let needs_scroll = count as f32 * (card_width + 9.) - 9. > available;
        let mut targets = row![].spacing(9);
        for (index, s) in self.sessions.iter().enumerate() {
            let selected = index == self.selected;
            let city = s
                .endpoint
                .as_ref()
                .and_then(|v| v.city.as_deref())
                .filter(|city| city.chars().count() <= 24)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{} {:02}", self.t(K::Target), index + 1));
            let target = if s.target.chars().count() > 28 {
                format!("{}…", s.target.chars().take(27).collect::<String>())
            } else {
                s.target.clone()
            };
            let progress = self
                .routes
                .iter()
                .find(|r| r.id == s.target_id)
                .filter(|r| r.nodes.len() > 1)
                .map_or(0., |r| r.progress);
            let card = button(
                column![
                    row![
                        text(format!("{:02}", index + 1)).size(11).color(p.green),
                        text(city).size(13).width(Fill)
                    ]
                    .spacing(7)
                    .align_y(iced::Center),
                    text(target).size(12).color(p.muted),
                    progress_bar(0.0..=1.0, progress).girth(2).style(move |_| {
                        iced::widget::progress_bar::Style {
                            background: p.line.into(),
                            bar: p.green.into(),
                            border: iced::Border::default(),
                        }
                    })
                ]
                .spacing(7),
            )
            .padding([10, 12])
            .width(if needs_scroll {
                Length::Fixed(card_width)
            } else {
                Fill
            })
            .on_press(Message::Select(index))
            .style(move |_, status| {
                if selected {
                    ui::selected(dark, status)
                } else {
                    ui::quiet(dark, status)
                }
            });
            // The full target and observation state remain available without
            // turning the animation strip into a second diagnostics panel.
            let detail = format!("{} · {}", s.target, self.t(self.session_status(s)));
            targets = targets.push(
                iced::widget::tooltip(
                    card,
                    text(detail).size(12),
                    iced::widget::tooltip::Position::Top,
                )
                .style(move |_| ui::card(dark)),
            );
        }
        if needs_scroll {
            scrollable(targets)
                .direction(iced::widget::scrollable::Direction::Horizontal(
                    iced::widget::scrollable::Scrollbar::default(),
                ))
                .spacing(6)
                .into()
        } else {
            targets.width(Fill).into()
        }
    }
}
