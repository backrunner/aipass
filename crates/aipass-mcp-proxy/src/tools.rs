use serde_json::{json, Value};

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "proxy_status",
            "description": "Get the current status of the proxy server",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_start",
            "description": "Start the proxy server",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_stop",
            "description": "Stop the proxy server",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_restart",
            "description": "Restart the proxy server",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_list_routes",
            "description": "List all proxy routes (groups)",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_create_route",
            "description": "Create a new proxy route (group)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the route"
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the route is enabled"
                    },
                    "inbound_protocol": {
                        "type": "string",
                        "enum": ["open_ai_responses", "open_ai_chat_completions", "anthropic_messages"],
                        "description": "Inbound protocol for the route"
                    },
                    "upstream_protocol": {
                        "type": "string",
                        "enum": ["open_ai_responses", "open_ai_chat_completions", "anthropic_messages"],
                        "description": "Upstream protocol for the route"
                    },
                    "conversion_enabled": {
                        "type": "boolean",
                        "description": "Whether protocol conversion is enabled"
                    },
                    "targets": {
                        "type": "array",
                        "description": "List of target providers",
                        "items": {
                            "type": "object",
                            "properties": {
                                "provider_id": {
                                    "type": "string"
                                },
                                "weight": {
                                    "type": "integer"
                                }
                            },
                            "required": ["provider_id"]
                        }
                    },
                    "strategy": {
                        "type": "string",
                        "enum": ["fallback", "round_robin"],
                        "description": "Load balancing strategy"
                    }
                },
                "required": ["name", "inbound_protocol", "upstream_protocol", "targets"]
            }
        }),
        json!({
            "name": "proxy_update_route",
            "description": "Update an existing proxy route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route to update"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the route"
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the route is enabled"
                    },
                    "inbound_protocol": {
                        "type": "string",
                        "enum": ["open_ai_responses", "open_ai_chat_completions", "anthropic_messages"],
                        "description": "Inbound protocol for the route"
                    },
                    "upstream_protocol": {
                        "type": "string",
                        "enum": ["open_ai_responses", "open_ai_chat_completions", "anthropic_messages"],
                        "description": "Upstream protocol for the route"
                    },
                    "conversion_enabled": {
                        "type": "boolean",
                        "description": "Whether protocol conversion is enabled"
                    },
                    "targets": {
                        "type": "array",
                        "description": "New list of targets"
                    },
                    "strategy": {
                        "type": "string",
                        "enum": ["fallback", "round_robin"]
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_delete_route",
            "description": "Delete a proxy route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route to delete"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_enable_route",
            "description": "Enable a proxy route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route to enable"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_disable_route",
            "description": "Disable a proxy route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route to disable"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_token_rotate",
            "description": "Rotate the authentication token for a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_usage",
            "description": "Get usage statistics for the proxy",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "days": {
                        "type": "integer",
                        "description": "Number of days to include"
                    },
                    "timezone_offset_minutes": {
                        "type": "integer",
                        "description": "Timezone offset in minutes"
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "proxy_logs",
            "description": "Get proxy server logs",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of log entries to return"
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "proxy_list_providers",
            "description": "List all available providers",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "proxy_get_provider",
            "description": "Get details of a specific provider",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider_id": {
                        "type": "string",
                        "description": "ID of the provider"
                    }
                },
                "required": ["provider_id"]
            }
        }),
        json!({
            "name": "proxy_create_provider",
            "description": "Create a new provider entry in the vault. The provider_data should match the ProviderEntryInput structure with fields like title, provider_kind, domains, endpoints, auth_scheme, etc.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider_data": {
                        "type": "object",
                        "description": "Complete provider entry data matching ProviderEntryInput structure"
                    }
                },
                "required": ["provider_data"]
            }
        }),
        json!({
            "name": "proxy_update_provider",
            "description": "Update an existing provider entry. The provider_data should match the ProviderEntryUpdateInput structure with optional fields to update.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entry_id": {
                        "type": "string",
                        "description": "ID of the provider entry to update"
                    },
                    "provider_data": {
                        "type": "object",
                        "description": "Provider update data matching ProviderEntryUpdateInput structure"
                    }
                },
                "required": ["entry_id", "provider_data"]
            }
        }),
        json!({
            "name": "proxy_delete_provider",
            "description": "Delete a provider",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider_id": {
                        "type": "string",
                        "description": "ID of the provider to delete"
                    }
                },
                "required": ["provider_id"]
            }
        }),
        json!({
            "name": "proxy_reorder_targets",
            "description": "Reorder and update weights of targets in a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "targets": {
                        "type": "array",
                        "description": "Ordered list of targets with optional weights; omitted targets are removed",
                        "items": {
                            "type": "object",
                            "properties": {
                                "target_id": {
                                    "type": "string"
                                },
                                "weight": {
                                    "type": "integer"
                                }
                            },
                            "required": ["target_id"]
                        }
                    }
                },
                "required": ["route_id", "targets"]
            }
        }),
        json!({
            "name": "proxy_list_groups",
            "description": "List all groups in a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_enable_group",
            "description": "Enable all targets in a group",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "group": {
                        "type": "string",
                        "description": "Name of the group"
                    }
                },
                "required": ["route_id", "group"]
            }
        }),
        json!({
            "name": "proxy_disable_group",
            "description": "Disable all targets in a group",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "group": {
                        "type": "string",
                        "description": "Name of the group"
                    }
                },
                "required": ["route_id", "group"]
            }
        }),
        json!({
            "name": "proxy_list_targets",
            "description": "List all targets in a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    }
                },
                "required": ["route_id"]
            }
        }),
        json!({
            "name": "proxy_enable_target",
            "description": "Enable a specific target",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "target_id": {
                        "type": "string",
                        "description": "ID of the target"
                    }
                },
                "required": ["route_id", "target_id"]
            }
        }),
        json!({
            "name": "proxy_disable_target",
            "description": "Disable a specific target",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "target_id": {
                        "type": "string",
                        "description": "ID of the target"
                    }
                },
                "required": ["route_id", "target_id"]
            }
        }),
        json!({
            "name": "proxy_set_target_priority",
            "description": "Set the priority of a target",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "target_id": {
                        "type": "string",
                        "description": "ID of the target"
                    },
                    "priority": {
                        "type": "integer",
                        "description": "Priority value (higher = more preferred)"
                    }
                },
                "required": ["route_id", "target_id", "priority"]
            }
        }),
        json!({
            "name": "proxy_set_target_weight",
            "description": "Set the weight of a target for load balancing",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "target_id": {
                        "type": "string",
                        "description": "ID of the target"
                    },
                    "weight": {
                        "type": "integer",
                        "description": "Weight value for load balancing"
                    }
                },
                "required": ["route_id", "target_id", "weight"]
            }
        }),
        json!({
            "name": "proxy_apply_to_tool",
            "description": "Apply proxy configuration to a specific tool (e.g., agent application)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route to apply"
                    },
                    "tool": {
                        "type": "string",
                        "description": "Tool/application name to apply the proxy config to"
                    }
                },
                "required": ["route_id", "tool"]
            }
        }),
        json!({
            "name": "proxy_switch_group",
            "description": "Switch to a specific group by disabling all targets and enabling only the specified group",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "group": {
                        "type": "string",
                        "description": "Name of the group to switch to"
                    }
                },
                "required": ["route_id", "group"]
            }
        }),
        json!({
            "name": "proxy_add_target",
            "description": "Add a new target to a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "provider_id": {
                        "type": "string",
                        "description": "ID of the provider entry to add"
                    },
                    "secret_id": {
                        "type": "string",
                        "description": "ID of the secret to use for authentication"
                    },
                    "group": {
                        "type": "string",
                        "description": "Optional group name for the target"
                    },
                    "priority": {
                        "type": "integer",
                        "description": "Priority value (default: 100)"
                    },
                    "weight": {
                        "type": "integer",
                        "description": "Weight value (default: 1)"
                    }
                },
                "required": ["route_id", "provider_id", "secret_id"]
            }
        }),
        json!({
            "name": "proxy_remove_target",
            "description": "Remove a target from a route",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "string",
                        "description": "ID of the route"
                    },
                    "target_id": {
                        "type": "string",
                        "description": "ID of the target to remove"
                    }
                },
                "required": ["route_id", "target_id"]
            }
        }),
    ]
}
