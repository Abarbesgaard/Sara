25 caught mutant(s), killed by 48 of 908 test(s).

Tests that kill something (kills, unique kills):
     4    0  /cite::cite_cli_bad_label_fails_atomically
     4    0  /cite::cite_cli_step_done_and_done_record_cited_uses   <- removable
     4    0  /cite::cite_info_lists_cited_memories   <- removable
     4    0  /cite::cite_mcp_step_done_and_done_record_and_reject_atomically   <- removable
     3    0  /cli_errors::secret_guardrail_rejects_and_exits_1   <- removable
     3    0  /cli_errors::unknown_subcommand_exits_2   <- removable
     3    0  /cli_errors::unknown_task_id_exits_1   <- removable
     3    0  /cli_json::info_json_shape   <- removable
     3    0  /cli_json::list_json_shape   <- removable
     3    0  /cli_json::recall_json_shape   <- removable
    15   12  /completion::completion_offers_task_ids_projects_and_memory_labels
     4    0  /doctor::doctor_human_report_lists_checks_and_verdict   <- removable
     3    0  /doctor::doctor_json_reports_a_healthy_fresh_store   <- removable
     4    0  /doctor::doctor_knowledge_reuse_scopes_to_the_current_project   <- removable
     4    0  /doctor::doctor_mcp_tool_returns_the_cli_report   <- removable
     3    0  /doctor::doctor_strict_exits_1_when_a_check_warns   <- removable
     1    0  /known_large_files_are_still_large   <- removable
     3    0  /links::link_then_unlink_round_trips_through_info   <- removable
     3    0  /links::linking_the_same_url_twice_keeps_one_link   <- removable
     4    0  /links::mcp_link_reports_already_linked   <- removable
     4    0  /links::mcp_unlink_by_link_id   <- removable
     4    0  /links::mcp_unlink_by_task_and_url   <- removable
     4    0  /links::mcp_unlink_needs_link_id_or_task_and_url   <- removable
     3    0  /links::unlink_unknown_id_fails   <- removable
     4    0  /mcp::begin_learn_recall_payloads   <- removable
     4    0  /mcp::find_tool_searches_tasks_by_partial_uuid   <- removable
     4    0  /mcp::tool_failure_is_an_is_error_result_not_a_protocol_error   <- removable
     4    0  /mcp::tool_response_envelope_is_wellformed   <- removable
     5    0  /provenance::provenance_memories_lists_track_record_in_text_and_json   <- removable
     4    0  /provenance::provenance_recall_shows_track_record_in_text_and_json   <- removable
     3    0  /recall::recall_task_cli_records_uses_only_when_a_task_is_given   <- removable
     3    0  /recall::recall_task_cli_rejects_an_unknown_task_before_recording   <- removable
     4    0  /recall::recall_task_mcp_records_uses_and_rejects_an_unknown_task   <- removable
     4    0  /recall::recall_text_covers_every_output_branch   <- removable
     4    0  /recall::recall_text_on_an_empty_store   <- removable
     4    0  /reflect::reflect_text_proposes_then_applies_a_cluster   <- removable
     4    0  /reflect::reflect_text_reports_nothing_on_an_empty_store   <- removable
     3    2  /shorthand::bare_id_and_id_first_forms_show_the_task_like_info
     3    0  /tag_parsing::add_trims_tags_and_drops_empty_ones   <- removable
     4    0  /tag_parsing::mcp_accepts_comma_separated_tags   <- removable
     3    0  /tag_parsing::modify_trims_tags_and_drops_empty_ones   <- removable
     3    1  /telemetry::clap_exits_are_captured
     4    0  /telemetry::every_mcp_tool_call_is_captured   <- removable
     6    0  /telemetry::outcome_cli_done_records_prior_memory_count   <- removable
     4    0  /telemetry::outcome_mcp_done_records_prior_memory_count   <- removable
     7    1  /telemetry::telemetry_command_does_not_record_itself
     3    0  /telemetry::telemetry_status_honours_the_env_opt_out   <- removable
     5    1  /undo::undo_reverts_the_last_command

42 test(s) can be removed together without losing a kill (marked above).
860 test(s) killed nothing in what was swept.
