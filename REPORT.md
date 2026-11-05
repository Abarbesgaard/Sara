Shards done: 4/128.
Partial: a test marked removable may still catch a mutant in a shard not yet swept.

68 caught mutant(s), killed by 48 of 932 test(s).

Tests that kill something (kills, unique kills):
     1    0  architecture/known_large_files_are_still_large   <- removable
     1    0  behaviour/cite::cite_cli_bad_label_fails_atomically   <- removable
     1    0  behaviour/cite::cite_cli_step_done_and_done_record_cited_uses   <- removable
     1    0  behaviour/cite::cite_info_lists_cited_memories   <- removable
     1    0  behaviour/cite::cite_mcp_step_done_and_done_record_and_reject_atomically   <- removable
     2    0  behaviour/cli_json::info_json_shape   <- removable
     2    0  behaviour/cli_json::list_json_shape   <- removable
     1    0  behaviour/provenance::provenance_memories_lists_track_record_in_text_and_json   <- removable
     1    0  behaviour/provenance::provenance_recall_shows_track_record_in_text_and_json   <- removable
     1    0  behaviour/recall::recall_task_cli_records_uses_only_when_a_task_is_given   <- removable
     1    0  behaviour/recall::recall_task_cli_rejects_an_unknown_task_before_recording   <- removable
     1    0  behaviour/recall::recall_task_mcp_records_uses_and_rejects_an_unknown_task   <- removable
     1    0  behaviour/recall::recall_text_covers_every_output_branch   <- removable
     1    0  behaviour/tag_parsing::add_trims_tags_and_drops_empty_ones   <- removable
     1    0  behaviour/tag_parsing::mcp_accepts_comma_separated_tags   <- removable
     1    0  behaviour/telemetry::outcome_cli_done_records_prior_memory_count   <- removable
     1    0  behaviour/telemetry::outcome_mcp_done_records_prior_memory_count   <- removable
     7    0  contract/add::an_added_task_is_pending   (Contract)
     5    0  contract/add::an_empty_description_is_refused   (Contract)
     2    0  contract/add::flags_win_over_inline_modifiers   (Contract)
     9    0  contract/add::modifiers_set_the_tasks_fields   (Contract)
     6    0  contract/ids::a_blank_id_matches_nothing   (Contract)
     6    0  contract/ids::a_short_number_is_a_display_id   (Contract)
     8    0  contract/ids::an_all_digit_uuid_prefix_is_a_uuid   (Contract)
     5    0  contract/ids::force_acts_on_any_branch   (Contract)
     9    0  contract/ids::the_branch_guard_lets_an_all_digit_uuid_prefix_through   (Contract)
    12    0  contract/ids::the_branch_guard_refuses_a_display_id   (Contract)
    12    0  contract/ids::the_suggested_id_works   (Contract)
    12    2  contract/undo::undo_reverts_the_last_command   (Contract)
     5    0  contract/validate::validate_explains_a_folder_outside_git   (Contract)
     9    0  contract/validate::validate_explains_a_moved_project   (Contract)
     9    0  contract/validate::validate_explains_a_repo_without_commits   (Contract)
     4    0  sara/commands::activity::render::tests::activity_drill_down_reports_an_empty_day   <- removable
     2    0  sara/commands::activity::render::tests::activity_grid_scrolls_back_to_keep_the_cursor_visible   <- removable
    29    0  sara/commands::activity::render::tests::styled_snapshot_activity   <- removable
    32    2  sara/commands::activity::render::tests::styled_snapshot_activity_drill_down
    29    0  sara/commands::activity::render::tests::styled_snapshot_activity_retro   <- removable
     3    0  sara/commands::activity::tests::activity_cursor_moves_by_day_and_week_and_stays_in_range
     3    0  sara/commands::activity::tests::activity_enter_drills_into_the_cursor_day_and_esc_closes_before_quitting   <- removable
     6    6  sara/commands::add::similar::tests::reordered_words_still_find_the_task_by_tokens
     2    2  sara/commands::add::tests::duplicate_open_task_detected_case_insensitively_same_project
     5    3  sara/commands::add::tests::form_with_guide_fields_founds_a_begin_equivalent_task
     2    0  sara/commands::add::tests::guide_assignment_defaults_to_the_title_like_begin   <- removable
     2    0  sara/commands::begin::tests::begin_folded_log_skips_operations_that_did_not_run   <- removable
     2    0  sara/commands::begin::tests::begin_founds_a_task_with_assignment_and_next_cursor   <- removable
     2    0  sara/commands::begin::tests::begin_records_every_folded_operation_as_an_ordered_event   <- removable
     6    0  sara/commands::begin::tests::begin_records_surfaced_use_against_the_new_task   <- removable
     2    0  sara/commands::begin::tests::missing_acceptance_warns_but_still_founds_the_task   <- removable

28 test(s) can be removed together without losing a kill (marked above).
884 test(s) killed nothing in what was swept.
