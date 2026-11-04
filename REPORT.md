71 caught mutant(s), killed by 75 of 930 test(s).

Tests that kill something (kills, unique kills):
     1    0  architecture/known_large_files_are_still_large   <- removable
     7    0  contract/add::an_added_task_is_pending   <- removable
     5    0  contract/add::an_empty_description_is_refused   <- removable
     7    0  contract/add::modifiers_set_the_tasks_fields   <- removable
     8    0  contract/cite::cite_cli_bad_label_fails_atomically
     8    0  contract/cite::cite_cli_step_done_and_done_record_cited_uses   <- removable
     8    0  contract/cite::cite_info_lists_cited_memories   <- removable
     8    0  contract/cite::cite_mcp_step_done_and_done_record_and_reject_atomically   <- removable
     5    0  contract/cli_errors::add_rejects_an_unknown_priority   <- removable
     3    0  contract/cli_errors::secret_guardrail_rejects_and_exits_1   <- removable
     3    0  contract/cli_errors::unknown_subcommand_exits_2   <- removable
     3    0  contract/cli_errors::unknown_task_id_exits_1   <- removable
    10    0  contract/cli_json::info_json_shape   <- removable
    10    0  contract/cli_json::list_json_shape   <- removable
     3    0  contract/cli_json::recall_json_shape   <- removable
    21   12  contract/completion::completion_offers_task_ids_projects_and_memory_labels
     4    0  contract/doctor::doctor_human_report_lists_checks_and_verdict   <- removable
     3    0  contract/doctor::doctor_json_reports_a_healthy_fresh_store   <- removable
     4    0  contract/doctor::doctor_knowledge_reuse_scopes_to_the_current_project   <- removable
     4    0  contract/doctor::doctor_mcp_tool_returns_the_cli_report   <- removable
     3    0  contract/doctor::doctor_strict_exits_1_when_a_check_warns   <- removable
     6    0  contract/ids::a_blank_id_matches_nothing   <- removable
     5    0  contract/ids::a_short_number_is_a_display_id   <- removable
     7    0  contract/ids::an_all_digit_uuid_prefix_is_a_uuid   <- removable
     5    0  contract/ids::force_acts_on_any_branch   <- removable
     9    0  contract/ids::the_branch_guard_lets_an_all_digit_uuid_prefix_through   <- removable
     9    0  contract/ids::the_branch_guard_refuses_a_display_id   <- removable
     9    0  contract/ids::the_suggested_id_works   <- removable
     5    0  contract/links::link_then_unlink_round_trips_through_info   <- removable
     5    0  contract/links::linking_the_same_url_twice_keeps_one_link   <- removable
     6    0  contract/links::mcp_link_reports_already_linked   <- removable
     6    0  contract/links::mcp_unlink_by_link_id   <- removable
     6    0  contract/links::mcp_unlink_by_task_and_url   <- removable
     4    0  contract/links::mcp_unlink_needs_link_id_or_task_and_url   <- removable
     3    0  contract/links::unlink_unknown_id_fails   <- removable
    10    0  contract/mcp::begin_learn_recall_payloads   <- removable
    11    0  contract/mcp::find_tool_searches_tasks_by_partial_uuid   <- removable
     4    0  contract/mcp::tool_failure_is_an_is_error_result_not_a_protocol_error   <- removable
     4    0  contract/mcp::tool_response_envelope_is_wellformed   <- removable
     9    0  contract/provenance::provenance_memories_lists_track_record_in_text_and_json   <- removable
     8    0  contract/provenance::provenance_recall_shows_track_record_in_text_and_json   <- removable
     7    0  contract/recall::recall_task_cli_records_uses_only_when_a_task_is_given   <- removable
     7    0  contract/recall::recall_task_cli_rejects_an_unknown_task_before_recording   <- removable
     8    0  contract/recall::recall_task_mcp_records_uses_and_rejects_an_unknown_task   <- removable
    12    0  contract/recall::recall_text_covers_every_output_branch   <- removable
     4    0  contract/recall::recall_text_on_an_empty_store   <- removable
     4    0  contract/reflect::reflect_text_proposes_then_applies_a_cluster   <- removable
     4    0  contract/reflect::reflect_text_reports_nothing_on_an_empty_store   <- removable
     5    2  contract/shorthand::bare_id_and_id_first_forms_show_the_task_like_info
     8    0  contract/tag_parsing::add_trims_tags_and_drops_empty_ones   <- removable
     9    0  contract/tag_parsing::mcp_accepts_comma_separated_tags   <- removable
     5    0  contract/tag_parsing::modify_trims_tags_and_drops_empty_ones   <- removable
     3    1  contract/telemetry::clap_exits_are_captured
     4    0  contract/telemetry::every_mcp_tool_call_is_captured   <- removable
    10    0  contract/telemetry::outcome_cli_done_records_prior_memory_count
     8    0  contract/telemetry::outcome_mcp_done_records_prior_memory_count   <- removable
     7    1  contract/telemetry::telemetry_command_does_not_record_itself
     3    0  contract/telemetry::telemetry_status_honours_the_env_opt_out   <- removable
    12    1  contract/undo::undo_reverts_the_last_command
     5    0  contract/validate::validate_explains_a_folder_outside_git   <- removable
     9    0  contract/validate::validate_explains_a_moved_project   <- removable
     9    0  contract/validate::validate_explains_a_repo_without_commits   <- removable
     4    0  sara/commands::activity::render::tests::activity_drill_down_reports_an_empty_day   <- removable
     2    0  sara/commands::activity::render::tests::activity_grid_scrolls_back_to_keep_the_cursor_visible   <- removable
    29    0  sara/commands::activity::render::tests::styled_snapshot_activity   <- removable
    32    2  sara/commands::activity::render::tests::styled_snapshot_activity_drill_down
    29    0  sara/commands::activity::render::tests::styled_snapshot_activity_retro   <- removable
     3    0  sara/commands::activity::tests::activity_cursor_moves_by_day_and_week_and_stays_in_range
     3    0  sara/commands::activity::tests::activity_enter_drills_into_the_cursor_day_and_esc_closes_before_quitting   <- removable
     2    2  sara/commands::add::tests::duplicate_open_task_detected_case_insensitively_same_project
     2    0  sara/commands::begin::tests::begin_folded_log_skips_operations_that_did_not_run   <- removable
     2    0  sara/commands::begin::tests::begin_founds_a_task_with_assignment_and_next_cursor   <- removable
     2    0  sara/commands::begin::tests::begin_records_every_folded_operation_as_an_ordered_event   <- removable
     5    0  sara/commands::begin::tests::begin_records_surfaced_use_against_the_new_task   <- removable
     2    0  sara/commands::begin::tests::missing_acceptance_warns_but_still_founds_the_task   <- removable

65 test(s) can be removed together without losing a kill (marked above).
855 test(s) killed nothing in what was swept.
