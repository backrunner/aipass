use super::*;
use aipass_agent_protocol::{
    AgentErrorCode, CredentialAssignment, PricingApplyScope, PricingConfig,
};
use aipass_desktop_native::pricing::{draft, Assignment, Operation, Request, Scope};
use uuid::Uuid;

fn request(
    transport: &mut AgentTransport,
    entry: Uuid,
    operation: Operation,
) -> Result<PricingConfig, aipass_agent::AgentCommandError> {
    let scope = Scope {
        entry,
        generation: 19,
    };
    let Reply::Pricing(completion) = transport
        .execute(Command::Pricing(Box::new(Request { scope, operation })))
        .unwrap()
    else {
        panic!("pricing reply");
    };
    assert_eq!(completion.scope, scope);
    completion.result
}
pub(super) fn workflow(transport: &mut AgentTransport, entry: Uuid, secret_id: &str) {
    let group_id = Uuid::new_v4();
    let mut rule = draft::Rule {
        model: "synthetic-".into(),
        prices: ["0", "0", "0", "0", ""].map(str::to_owned),
        ..Default::default()
    };
    rule.prices[0] = "2.5".into();
    rule.prices[4] = "0".into();
    let group = draft::group(group_id, "Native price rules", &[rule.clone()]).unwrap();
    let config = request(
        transport,
        entry,
        Operation::SaveGroup {
            group,
            apply_scope: PricingApplyScope::AllHistory,
        },
    )
    .unwrap();
    assert_eq!(
        config
            .groups
            .iter()
            .find(|g| g.id == group_id)
            .unwrap()
            .versions[0]
            .rules[0]
            .cache_creation_1h_micros_per_million,
        Some(0)
    );
    let assignment = Assignment {
        entry_id: entry,
        secret_id: secret_id.to_owned(),
        group_id: Some(group_id),
        multiplier: 0.5,
    };
    let config = request(transport, entry, Operation::Assign(assignment.clone())).unwrap();
    assert!(config.assignments.iter().any(|a| a.entry_id == entry
        && a.secret_id == secret_id
        && a.group_id == Some(group_id)
        && a.multiplier == 0.5
        && a.manual));
    rule.prices[0] = "3.5".into();
    let group = draft::group(group_id, "Native price rules", &[rule]).unwrap();
    let config = request(
        transport,
        entry,
        Operation::SaveGroup {
            group,
            apply_scope: PricingApplyScope::FromNow,
        },
    )
    .unwrap();
    let versions = &config
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .unwrap()
        .versions;
    assert_eq!(versions.len(), 2);
    let at = versions.iter().map(|v| v.effective_from).max().unwrap();
    assert!(at > 0);
    request(
        transport,
        entry,
        Operation::DeleteVersion {
            group_id,
            effective_from: at,
        },
    )
    .unwrap();
    transport.execute(Command::Lock).unwrap();
    assert_eq!(
        request(transport, entry, Operation::Assign(assignment))
            .unwrap_err()
            .code,
        Some(AgentErrorCode::Locked)
    );
    transport
        .execute(Command::Unlock("synthetic-native-workflow-password".into()))
        .unwrap();
    let config = request(transport, entry, Operation::Load).unwrap();
    assert_eq!(
        config
            .groups
            .iter()
            .find(|g| g.id == group_id)
            .unwrap()
            .versions
            .len(),
        1
    );
    assert!(config
        .assignments
        .iter()
        .any(|a| a.entry_id == entry && a.secret_id == secret_id && a.multiplier == 0.5));
    let config = request(transport, entry, Operation::DeleteGroup(group_id)).unwrap();
    assert!(!config.groups.iter().any(|g| g.id == group_id));
    assert!(!config
        .assignments
        .iter()
        .any(|a: &CredentialAssignment| a.group_id == Some(group_id)));
    let mut unknown = Assignment {
        entry_id: entry,
        secret_id: "missing-credential".into(),
        group_id: None,
        multiplier: 1.,
    };
    assert!(request(transport, entry, Operation::Assign(unknown.clone())).is_err());
    unknown.secret_id = secret_id.into();
    unknown.multiplier = -1.;
    assert!(request(transport, entry, Operation::Assign(unknown)).is_err());
}
