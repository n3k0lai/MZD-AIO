//! `mzd-tweaks-mcp`: MCP server over stdio exposing the mzd-tweaks catalog, planner and
//! bundle builder. Writes and serial access are dry runs unless explicitly disabled.

mod tools;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use tools::*;

const INSTRUCTIONS: &str = "Turn a driver's request into a validated Mazda Connect CMU tweak bundle. \
Always: check_compat with the exact firmware first, then plan_bundle and show the user the manifest \
and warnings, then build_bundle with dry_run true, and only with the user's go-ahead dry_run false. \
Bundles default to mode \"check\" (no changes on the CMU); use \"apply\" only after a check run passed. \
Never invent firmware versions. Tweaks that reduce driving safety need the user's explicit acknowledgement.";

#[derive(Debug, Clone)]
pub struct Server {
    tool_router: ToolRouter<Self>,
}

fn out(r: ToolResult) -> Result<String, String> {
    r.map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
}

#[tool_router]
impl Server {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List CMU tweaks with id, status, risk and whether they need an acknowledgement."
    )]
    async fn list_tweaks(&self, Parameters(a): Parameters<ListArgs>) -> Result<String, String> {
        out(list_tweaks(a))
    }

    #[tool(
        description = "Full catalog entry for one tweak: options, compatibility, steps, notes, upstream sources."
    )]
    async fn describe_tweak(&self, Parameters(a): Parameters<IdArgs>) -> Result<String, String> {
        out(describe_tweak(a))
    }

    #[tool(
        description = "Which firmware profile a CMU version falls in and which tweaks can be built for it."
    )]
    async fn check_compat(&self, Parameters(a): Parameters<CompatArgs>) -> Result<String, String> {
        out(compat(a))
    }

    #[tool(
        description = "Validate a bundle config and return the manifest it would produce (files touched, backups, space, warnings). Writes nothing."
    )]
    async fn plan_bundle(&self, Parameters(a): Parameters<PlanArgs>) -> Result<String, String> {
        out(plan_bundle(a))
    }

    #[tool(
        description = "Build the USB bundle into out_dir. dry_run defaults to true; set it false only after the user approved the plan."
    )]
    async fn build_bundle(&self, Parameters(a): Parameters<BuildArgs>) -> Result<String, String> {
        out(build_bundle(a))
    }

    #[tool(
        description = "Read-only probes (version, df, uptime) over the bench CMU serial console. Requires an explicit device path; dry_run defaults to true."
    )]
    async fn serial_probe(&self, Parameters(a): Parameters<ProbeArgs>) -> Result<String, String> {
        out(serial_probe(a))
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mzd-tweaks", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let service = Server::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_expected_tools() {
        let mut names: Vec<String> = Server::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "build_bundle",
                "check_compat",
                "describe_tweak",
                "list_tweaks",
                "plan_bundle",
                "serial_probe"
            ]
        );
    }
}
