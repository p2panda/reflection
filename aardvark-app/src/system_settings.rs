/* system_settings.rs
 *
 * Copyright 2025 The Aardvark Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use adw::prelude::*;
use adw::subclass::prelude::*;
#[cfg(target_os = "linux")]
use ashpd::{desktop::settings::Settings as SettingsProxy, zvariant};
#[cfg(target_os = "linux")]
use futures_util::stream::StreamExt;
use gtk::{glib, glib::Properties, glib::clone, pango};
use std::cell::{Cell, RefCell};
use tracing::error;

#[cfg(target_os = "linux")]
const GNOME_DESKTOP_NAMESPACE: &str = "org.gnome.desktop.interface";

#[cfg(target_os = "linux")]
const CLOCK_FORMAT_KEY: &str = "clock-format";

#[cfg(target_os = "linux")]
const MONOSPACE_FONT_NAME_KEY: &str = "monospace-font-name";

/// The clock format setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[repr(u32)]
#[enum_type(name = "ClockFormat")]
pub enum ClockFormat {
    /// The 12h format, i.e. AM/PM.
    TwelveHours = 0,
    /// The 24h format.
    TwentyFourHours = 1,
}

impl Default for ClockFormat {
    fn default() -> Self {
        // Use the locale's default clock format as a fallback.
        let local_formatted_time = glib::DateTime::now_local()
            .and_then(|d| d.format("%X"))
            .map(|s| s.to_ascii_lowercase());
        match &local_formatted_time {
            Ok(s) if s.ends_with("am") || s.ends_with("pm") => ClockFormat::TwelveHours,
            Ok(_) => ClockFormat::TwentyFourHours,
            Err(error) => {
                error!("Could not get local formatted time: {error}");
                ClockFormat::TwelveHours
            }
        }
    }
}

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::SystemSettings)]
    pub struct SystemSettings {
        /// The clock format setting.
        #[property(get, builder(ClockFormat::default()))]
        pub clock_format: Cell<ClockFormat>,
        // The monospace font name setting.
        #[property(get)]
        pub monospace_font_name: RefCell<Option<pango::FontDescription>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SystemSettings {
        const NAME: &'static str = "SystemSettings";
        type Type = super::SystemSettings;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SystemSettings {
        fn constructed(&self) {
            self.parent_constructed();

            glib::spawn_future_local(clone!(
                #[weak(rename_to = this)]
                self,
                async move {
                    if let Err(error) = this.init().await {
                        #[cfg(target_os = "linux")]
                        error!("Unable to read system settings: {error}");

                        #[cfg(target_os = "macos")]
                        let _ = error;
                    }
                }
            ));
        }
    }

    impl SystemSettings {
        #[cfg(target_os = "linux")]
        async fn init(&self) -> Result<(), ashpd::Error> {
            let proxy = SettingsProxy::new().await?;
            let settings = proxy.read_all(&[GNOME_DESKTOP_NAMESPACE]).await?;

            if let Some(namespace) = settings.get(GNOME_DESKTOP_NAMESPACE) {
                if let Some(clock_format) = namespace.get(CLOCK_FORMAT_KEY) {
                    match ClockFormat::try_from(clock_format) {
                        Ok(clock_format) => {
                            self.set_clock_format(clock_format);
                        }
                        Err(error) => {
                            error!("Unable to read clock format system setting: {error}");
                            self.set_clock_format(ClockFormat::default());
                        }
                    };
                }
                if let Some(font_name) = namespace.get(MONOSPACE_FONT_NAME_KEY) {
                    match <&str>::try_from(font_name)
                        .and_then(|font_name| Ok(pango::FontDescription::from_string(font_name)))
                    {
                        Ok(font) => {
                            self.set_monospace_font_name(Some(font));
                        }
                        Err(error) => {
                            error!("Unable to read monofont system setting: {error}");
                            self.set_monospace_font_name(None);
                        }
                    };
                }
            }

            let setting_changed_stream = proxy.receive_setting_changed().await?;

            let obj_weak = self.obj().downgrade();
            setting_changed_stream
                .for_each(move |setting| {
                    let obj_weak = obj_weak.clone();
                    async move {
                        if setting.namespace() != GNOME_DESKTOP_NAMESPACE {
                            return;
                        }

                        if let Some(obj) = obj_weak.upgrade() {
                            if setting.key() == CLOCK_FORMAT_KEY {
                                match ClockFormat::try_from(setting.value()) {
                                    Ok(clock_format) => {
                                        obj.imp().set_clock_format(clock_format);
                                    }
                                    Err(error) => {
                                        error!(
                                            "Unable to read clock format system setting: {error}"
                                        );
                                        obj.imp().set_clock_format(ClockFormat::default());
                                    }
                                };
                            }

                            if setting.key() == MONOSPACE_FONT_NAME_KEY {
                                match <&str>::try_from(setting.value()).and_then(|font_name| {
                                    Ok(pango::FontDescription::from_string(font_name))
                                }) {
                                    Ok(font) => {
                                        obj.imp().set_monospace_font_name(Some(font));
                                    }
                                    Err(error) => {
                                        error!("Unable to read monofont system setting: {error}");
                                        obj.imp().set_monospace_font_name(None);
                                    }
                                };
                            }
                        }
                    }
                })
                .await;

            Ok(())
        }

        #[cfg(target_os = "macos")]
        async fn init(&self) -> Result<(), ()> {
            // TODO: Implement reading macOS system settings
            Ok(())
        }

        #[cfg(target_os = "linux")]
        fn set_clock_format(&self, clock_format: ClockFormat) {
            if self.obj().clock_format() == clock_format {
                return;
            }

            self.clock_format.set(clock_format);
            self.obj().notify_clock_format();
        }

        #[cfg(target_os = "linux")]
        fn set_monospace_font_name(&self, font_name: Option<pango::FontDescription>) {
            if self.obj().monospace_font_name() == font_name {
                return;
            }

            self.monospace_font_name.replace(font_name);
            self.obj().notify_monospace_font_name();
        }
    }
}

glib::wrapper! {
    pub struct SystemSettings(ObjectSubclass<imp::SystemSettings>);
}

impl SystemSettings {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<&zvariant::OwnedValue> for ClockFormat {
    type Error = zvariant::Error;

    fn try_from(value: &zvariant::OwnedValue) -> Result<Self, Self::Error> {
        let Ok(s) = <&str>::try_from(value) else {
            return Err(zvariant::Error::IncorrectType);
        };

        match s {
            "12h" => Ok(Self::TwelveHours),
            "24h" => Ok(Self::TwentyFourHours),
            _ => Err(zvariant::Error::Message(format!(
                "Invalid string `{s}`, expected `12h` or `24h`"
            ))),
        }
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<zvariant::OwnedValue> for ClockFormat {
    type Error = zvariant::Error;

    fn try_from(value: zvariant::OwnedValue) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}
