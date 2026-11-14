//! Subscription presentation contracts and authenticated Agent operations.
//! Credentials, account ownership and quota interpretation remain in the Agent.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, ClaudeCliStatus, ClaudeLoginStatus, CommunityLoginInput, CommunityLoginStatus,
    SensitiveString, SubscriptionImportInput, SubscriptionImportTask,
};
use aipass_provider_registry::SubscriptionSnapshot;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod links;
pub mod options;
pub mod presentation;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub name_key: Option<String>,
    #[serde(default)]
    pub local_authorization: bool,
    #[serde(default)]
    pub methods: Vec<Method>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Method {
    pub index: usize,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub label_key: Option<String>,
    pub hint_key: Option<String>,
    #[serde(default)]
    pub native: bool,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
    pub message_key: Option<String>,
    pub placeholder: Option<String>,
    #[serde(default)]
    pub retain: bool,
    #[serde(default)]
    pub options: Vec<Choice>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub label_key: Option<String>,
}

pub fn cli_owned(provider: &str) -> bool {
    matches!(
        provider,
        "claude" | "codex" | "grok" | "copilot" | "gemini-cli"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Claude(Uuid),
    Community(Uuid),
    Import(Uuid),
}
impl Flow {
    fn poll(self) -> AgentRequest {
        match self {
            Self::Claude(ticket) => AgentRequest::ClaudeLoginPoll { ticket },
            Self::Community(ticket) => AgentRequest::CommunityLoginPoll { ticket },
            Self::Import(ticket) => AgentRequest::SubscriptionImportPoll { ticket },
        }
    }
    fn cancel(self) -> AgentRequest {
        match self {
            Self::Claude(ticket) => AgentRequest::ClaudeLoginCancel { ticket },
            Self::Community(ticket) => AgentRequest::CommunityLoginCancel { ticket },
            Self::Import(ticket) => AgentRequest::SubscriptionImportCancel { ticket },
        }
    }
}

pub enum Operation {
    Options(Uuid, options::Action),
    Catalog,
    Cli(String),
    ClaudeStart,
    Start(CommunityLoginInput),
    Poll(Flow),
    Cancel(Flow),
    Code(Flow, SensitiveString),
    Refresh(Uuid),
    OpenLogin(Flow),
    OpenPublic(String),
    Console(Uuid, String),
    Install(String),
    Import(SubscriptionImportInput),
    OpenLocation { ticket: Uuid, source_id: Uuid },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Options(Uuid),
    Catalog,
    Cli,
    Start,
    Poll,
    Cancel,
    Code,
    Refresh(Uuid),
    Console(Uuid),
    External,
    Import,
}
impl Operation {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Options(id, _) => Kind::Options(*id),
            Self::Catalog => Kind::Catalog,
            Self::Cli(_) => Kind::Cli,
            Self::ClaudeStart | Self::Start(_) => Kind::Start,
            Self::Poll(_) => Kind::Poll,
            Self::Cancel(_) => Kind::Cancel,
            Self::Code(..) => Kind::Code,
            Self::Refresh(id) => Kind::Refresh(*id),
            Self::Import(_) => Kind::Import,
            Self::Console(id, _) => Kind::Console(*id),
            Self::OpenLogin(_)
            | Self::OpenPublic(_)
            | Self::Install(_)
            | Self::OpenLocation { .. } => Kind::External,
        }
    }
}
pub struct Request {
    pub generation: u64,
    pub operation: Operation,
}
pub struct Completion {
    pub generation: u64,
    pub kind: Kind,
    pub result: Result<Outcome, AgentCommandError>,
}
pub enum Outcome {
    Options(options::ResultData),
    Catalog(Vec<Provider>),
    Cli(ClaudeCliStatus),
    Login(Login),
    Import(SubscriptionImportTask),
    Refreshed(SubscriptionSnapshot),
    Done,
}
#[derive(Clone)]
pub struct Login {
    pub flow: Flow,
    pub pending: bool,
    pub entry_id: Option<Uuid>,
    pub url: Option<String>,
    pub code_required: bool,
    pub user_code: Option<String>,
    pub instructions: Option<String>,
    pub error: Option<String>,
}
impl Login {
    fn claude(value: ClaudeLoginStatus) -> Self {
        Self {
            flow: Flow::Claude(value.ticket),
            pending: value.status == "pending",
            entry_id: value.entry_id,
            code_required: value.url.is_some(),
            url: value.url,
            user_code: None,
            instructions: None,
            error: match value.status.as_str() {
                "expired" => Some("oauthConnect.expired".into()),
                "error" => Some("claudeConnect.signInFailed".into()),
                _ => None,
            },
        }
    }
    fn community(value: CommunityLoginStatus) -> Self {
        Self {
            flow: Flow::Community(value.ticket),
            pending: value.status == "pending",
            entry_id: value.entry_id,
            url: value.url,
            code_required: value.method.as_deref() == Some("code"),
            user_code: value.user_code,
            instructions: value.instructions,
            error: value.error,
        }
    }
}

fn poll(client: &AgentClient, flow: Flow) -> Result<Outcome, AgentCommandError> {
    match flow {
        Flow::Claude(_) => client
            .request(&flow.poll())
            .map(Login::claude)
            .map(Outcome::Login),
        Flow::Community(_) => client
            .request(&flow.poll())
            .map(Login::community)
            .map(Outcome::Login),
        Flow::Import(_) => client.request(&flow.poll()).map(Outcome::Import),
    }
}
impl Request {
    pub fn execute(self, client: &AgentClient) -> Completion {
        let kind = self.operation.kind();
        Completion {
            generation: self.generation,
            kind,
            result: execute(self.operation, client),
        }
    }
}
fn execute(operation: Operation, client: &AgentClient) -> Result<Outcome, AgentCommandError> {
    match operation {
        Operation::Options(id, action) => {
            options::execute(id, action, client).map(Outcome::Options)
        }
        Operation::Catalog => client
            .request(&AgentRequest::CommunityCatalog)
            .map(Outcome::Catalog),
        Operation::Cli(provider) => client
            .request(&if provider == "claude" {
                AgentRequest::ClaudeCliStatus
            } else {
                AgentRequest::SubscriptionCliStatus { provider }
            })
            .map(Outcome::Cli),
        Operation::ClaudeStart => client
            .request(&AgentRequest::ClaudeLoginStart)
            .map(Login::claude)
            .map(Outcome::Login),
        Operation::Start(input) => client
            .request(&AgentRequest::CommunityLoginStart { input })
            .map(Login::community)
            .map(Outcome::Login),
        Operation::Poll(flow) => poll(client, flow),
        Operation::Cancel(flow) => {
            let _: serde_json::Value = client.request(&flow.cancel())?;
            // Account persistence can win cancellation; do not hide a committed account.
            poll(client, flow)
        }
        Operation::Code(flow, code) => {
            let request = match flow {
                Flow::Claude(ticket) => AgentRequest::ClaudeLoginCode { ticket, code },
                Flow::Community(ticket) => AgentRequest::CommunityLoginCode { ticket, code },
                Flow::Import(_) => return Err(links::error("Import does not accept a code")),
            };
            let _: serde_json::Value = client.request(&request)?;
            Ok(Outcome::Done)
        }
        Operation::Refresh(entry_id) => client
            .request(&AgentRequest::CommunityRefresh { entry_id })
            .map(Outcome::Refreshed),
        Operation::Import(input) => client
            .request(&AgentRequest::SubscriptionImportStart { input })
            .map(Outcome::Import),
        Operation::OpenLogin(flow) => {
            let Outcome::Login(login) = poll(client, flow)? else {
                return Err(links::error("Not a login"));
            };
            if !login.pending {
                return Err(links::error("Sign-in is no longer pending"));
            }
            let uri = login
                .url
                .ok_or_else(|| links::error("Sign-in has no browser link"))?;
            links::open(links::authorization(&uri, matches!(flow, Flow::Claude(_)))?)?;
            Ok(Outcome::Done)
        }
        Operation::OpenPublic(uri) => {
            links::open(links::public(&uri)?)?;
            Ok(Outcome::Done)
        }
        Operation::Console(_, uri) => {
            links::open(links::public(&uri)?)?;
            Ok(Outcome::Done)
        }
        Operation::Install(provider) => {
            links::open(links::installation(&provider)?)?;
            Ok(Outcome::Done)
        }
        Operation::OpenLocation { ticket, source_id } => {
            let task: SubscriptionImportTask =
                client.request(&AgentRequest::SubscriptionImportPoll { ticket })?;
            let path = task
                .results
                .iter()
                .find(|result| result.source_id == source_id)
                .map(|result| result.source.root.clone())
                .ok_or_else(|| links::error("Unknown import source"))?;
            links::open_location(&path)?;
            Ok(Outcome::Done)
        }
    }
}
