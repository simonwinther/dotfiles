//! Authentication requests remain in memory and share the notch's focus owner.
use crate::{draw, layout::Rect, text::Text, theme::Theme};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

#[derive(Clone)]
pub struct Request {
    pub question: String,
    pub secret: bool,
    pub response: async_channel::Sender<Option<String>>,
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("question", &self.question)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}
#[derive(Clone)]
pub struct Model {
    pub request: Request,
    pub value: String,
    pub previous: Box<crate::panel::Kind>,
}
impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}
impl Model {
    pub fn draw(&self, p: &mut tiny_skia::Pixmap, text: &mut Text, theme: &Theme, s: f32) {
        let clip = Rect::new(28.0, 14.0, 424.0, 136.0).scale(s);
        text.draw_clipped(
            p,
            &self.request.question,
            28.0 * s,
            36.0 * s,
            12.0 * s,
            theme.text(),
            clip,
        );
        draw::rounded(
            p,
            Rect::new(28.0, 52.0, 424.0, 38.0).scale(s),
            9.0 * s,
            theme.text().alpha(0.09),
        );
        let value = if self.request.secret {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        text.draw_clipped(
            p,
            &value,
            42.0 * s,
            77.0 * s,
            13.0 * s,
            theme.text(),
            Rect::new(42.0, 52.0, 396.0, 38.0).scale(s),
        );
        text.draw(
            p,
            "Enter  confirm · Esc  cancel",
            28.0 * s,
            119.0 * s,
            10.0 * s,
            theme.dim(),
        );
    }
}
#[derive(Clone)]
pub struct Agent {
    pub sender: Sender<crate::modules::Update>,
}
impl Agent {
    async fn ask(&self, question: String, secret: bool) -> zbus::fdo::Result<String> {
        let (response, answers) = async_channel::bounded(1);
        self.sender
            .send(crate::modules::Update::Prompt(Request {
                question,
                secret,
                response,
            }))
            .map_err(|_| zbus::fdo::Error::Failed("Desktop closed".into()))?;
        answers
            .recv()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| zbus::fdo::Error::Failed("Cancelled".into()))
    }
}
#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn request_pin_code(
        &self,
        device: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<String> {
        self.ask(
            format!(
                "Bluetooth PIN · {}",
                device.as_str().rsplit('/').next().unwrap_or("Device")
            ),
            true,
        )
        .await
    }
    async fn request_passkey(
        &self,
        _device: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<u32> {
        let value = self.ask("Bluetooth pairing code".into(), false).await?;
        value
            .parse::<u32>()
            .ok()
            .filter(|v| *v <= 999999)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Enter a six-digit code".into()))
    }
    async fn request_confirmation(
        &self,
        _device: zbus::zvariant::ObjectPath<'_>,
        passkey: u32,
    ) -> zbus::fdo::Result<()> {
        self.ask(
            format!("Confirm Bluetooth pairing code {passkey:06}"),
            false,
        )
        .await
        .map(|_| ())
    }
    async fn request_authorization(
        &self,
        _device: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<()> {
        self.ask("Allow Bluetooth pairing?".into(), false)
            .await
            .map(|_| ())
    }
    async fn authorize_service(
        &self,
        _device: zbus::zvariant::ObjectPath<'_>,
        _uuid: &str,
    ) -> zbus::fdo::Result<()> {
        Ok(())
    }
    fn display_pin_code(&self, _device: zbus::zvariant::ObjectPath<'_>, _pincode: &str) {}
    fn display_passkey(
        &self,
        _device: zbus::zvariant::ObjectPath<'_>,
        _passkey: u32,
        _entered: u16,
    ) {
    }
    fn cancel(&self) {}
    fn release(&self) {}
}
#[derive(Clone)]
pub struct WirelessAgent(pub Agent);
#[zbus::interface(name = "net.connman.iwd.Agent")]
impl WirelessAgent {
    async fn request_passphrase(
        &self,
        _network: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<String> {
        self.0.ask("Wi-Fi password".into(), true).await
    }
    async fn request_private_key_passphrase(
        &self,
        _network: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<String> {
        self.0.ask("Wi-Fi private key password".into(), true).await
    }
    async fn request_user_name_and_password(
        &self,
        _network: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<(String, String)> {
        let username = self.0.ask("Wi-Fi username".into(), false).await?;
        let password = self.0.ask("Wi-Fi password".into(), true).await?;
        Ok((username, password))
    }
    async fn request_user_password(
        &self,
        _network: zbus::zvariant::ObjectPath<'_>,
        username: &str,
    ) -> zbus::fdo::Result<String> {
        self.0
            .ask(format!("Wi-Fi password · {username}"), true)
            .await
    }
    fn cancel(&self, _reason: &str) {}
    fn release(&self) {}
}
