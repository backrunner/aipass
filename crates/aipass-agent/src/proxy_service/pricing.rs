//! Usage accounting and credential pricing assignments.
use super::*;

impl ProxyService {
    pub fn usage_summary(
        &self,
        since: Option<i64>,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> ServiceResult<ServerUsageSummary> {
        let summary = self
            .usage
            .summary_since(since, self.cost_resolver(pricing, list_prices))
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
        Ok(ServerUsageSummary {
            request_count: summary.request_count,
            input_tokens: summary.input_tokens,
            output_tokens: summary.output_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            cache_creation_tokens: summary.cache_creation_tokens,
            estimated_cost_micros: summary.estimated_cost_micros,
            attempt_count: summary.attempt_count,
            completed_attempts: summary.completed_attempts,
            successful_attempts: summary.successful_attempts,
            success_rate_bps: summary.success_rate_bps,
            average_first_token_ms: summary.average_first_token_ms,
            providers: summary.providers,
            models: summary.models,
        })
    }

    pub fn clear_usage(&self) -> ServiceResult<()> {
        self.usage
            .clear()
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }

    pub fn usage_timeseries(
        &self,
        days: u32,
        timezone_offset_minutes: i32,
        granularity: UsageGranularity,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> ServiceResult<Vec<UsageTimeseriesPoint>> {
        self.usage
            .timeseries(
                days,
                timezone_offset_minutes,
                granularity,
                self.cost_resolver(pricing, list_prices),
            )
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }

    pub(super) fn cost_resolver(
        &self,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> impl Fn(&UsageRow) -> u64 {
        let config = pricing.clone();
        let overrides = self.config.pricing.clone();
        let list_prices = list_prices.to_vec();
        move |row: &UsageRow| {
            crate::pricing::resolve_cost(
                &config,
                &overrides,
                &list_prices,
                row.provider_entry_id,
                &row.secret_id,
                row.model.as_deref(),
                row.started_at,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_creation_tokens,
            )
        }
    }

    pub fn pricing_config(&self, vault: &Vault) -> ServiceResult<PricingConfig> {
        crate::pricing::load_pricing_config(&self.vault_dir, vault)
    }

    pub fn set_pricing_assignment(
        &self,
        vault: &Vault,
        entry_id: Uuid,
        secret_id: String,
        group_id: Option<Uuid>,
        multiplier: f64,
    ) -> ServiceResult<PricingConfig> {
        if !multiplier.is_finite() || multiplier < 0.0 {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "pricing multiplier must be a finite non-negative number",
            ));
        }
        let entry = vault
            .get_provider_summary(entry_id)
            .map_err(map_vault_error)?;
        if !entry
            .secret_refs
            .iter()
            .any(|secret| secret.id == secret_id)
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::NotFound,
                "pricing credential no longer exists",
            ));
        }
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        if group_id.is_some_and(|id| !config.groups.iter().any(|group| group.id == id)) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::NotFound,
                "pricing group no longer exists",
            ));
        }
        match config
            .assignments
            .iter_mut()
            .find(|item| item.entry_id == entry_id && item.secret_id == secret_id)
        {
            Some(existing) => {
                existing.group_id = group_id;
                existing.multiplier = multiplier;
                existing.manual = true;
            }
            None => config.assignments.push(CredentialAssignment {
                entry_id,
                secret_id,
                group_id,
                multiplier,
                manual: true,
            }),
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn upsert_pricing_group(
        &self,
        vault: &Vault,
        group: PricingGroup,
        apply_scope: PricingApplyScope,
    ) -> ServiceResult<PricingConfig> {
        crate::pricing::validate_group(&group)?;
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        let mut group = group;
        group.manual = true;
        match apply_scope {
            PricingApplyScope::AllHistory => {
                // All history is repriced with the incoming rule set: collapse
                // every supplied version to the epoch and replace the group.
                for version in &mut group.versions {
                    version.effective_from = 0;
                }
                normalize_versions(&mut group.versions);
                match config.groups.iter_mut().find(|item| item.id == group.id) {
                    Some(existing) => *existing = group,
                    None => config.groups.push(group),
                }
            }
            PricingApplyScope::FromNow => {
                // History keeps its prices: the incoming rules take effect now
                // and are appended to the group's version timeline.
                let now = OffsetDateTime::now_utc().unix_timestamp();
                for version in &mut group.versions {
                    version.effective_from = now;
                }
                match config.groups.iter_mut().find(|item| item.id == group.id) {
                    Some(existing) => {
                        existing.name = group.name;
                        existing.manual = true;
                        existing.versions.extend(group.versions);
                        normalize_versions(&mut existing.versions);
                    }
                    None => {
                        normalize_versions(&mut group.versions);
                        config.groups.push(group);
                    }
                }
            }
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn delete_pricing_group(
        &self,
        vault: &Vault,
        group_id: Uuid,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        config.groups.retain(|group| group.id != group_id);
        for assignment in &mut config.assignments {
            if assignment.group_id == Some(group_id) {
                assignment.group_id = None;
                assignment.manual = true;
            }
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn delete_pricing_group_version(
        &self,
        vault: &Vault,
        group_id: Uuid,
        effective_from: i64,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        let group = config
            .groups
            .iter_mut()
            .find(|group| group.id == group_id)
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::NotFound,
                    "pricing group not found",
                )
            })?;
        group.manual = true;
        group
            .versions
            .retain(|version| version.effective_from != effective_from);
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }
}
