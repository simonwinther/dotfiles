use anyhow::{Result, bail};
use desktop_ring::{
    core::{self, Action, Request},
    monitors::Position,
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("menu") {
        #[cfg(feature = "ui")]
        {
            return desktop_ring::ui::run();
        }
        #[cfg(not(feature = "ui"))]
        {
            bail!("built without the ui feature");
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(async {
        let request=match args.first().map(String::as_str) {
            Some("daemon")=>return core::daemon(core::config::load()?).await,
            Some("status")=>Request::Status,
            Some("rescan")=>Request::Act{action:Action::Rescan},
            Some("cycle")=>Request::Act{action:Action::Cycle},
            Some("layout")=>{
                let position=match args.get(1).map(String::as_str){Some("left")=>Position::Left,Some("right")=>Position::Right,Some("below")=>Position::Below,_=>bail!("layout: left, right, or below")};
                Request::Act{action:Action::Layout{position}}
            },
            Some("action")=>Request::Act{action:serde_json::from_str(args.get(1).ok_or_else(||anyhow::anyhow!("action requires a JSON argument"))?)?},
            _=>{println!("desktop-ring daemon | menu | status | rescan | cycle | layout left/right/below | action JSON");return Ok(());}
        };
        let response=core::client(&request).await?;
        if !response.ok{bail!("{}",response.error.unwrap_or_default());}
        println!("{}",serde_json::to_string_pretty(&response.snapshot)?);Ok(())
    })
}
