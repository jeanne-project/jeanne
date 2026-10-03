use jeanne_core::productivity::{
    append_bookmark, append_log_entry, append_todo, create_meeting_note, evaluate_math_expression,
    extract_tasks_from_file, toggle_task_in_file,
};
use tempfile::tempdir;

#[test]
fn test_prod_01_evaluate_simple_math() {
    let res = evaluate_math_expression("12 * 4.5").expect("Should evaluate valid arithmetic");
    assert!((res - 54.0).abs() < 1e-6);
}

#[test]
fn test_prod_01b_evaluate_float_precision_and_commas() {
    // Cas signalé par l'utilisateur : 1.2 * 56.4 doit valoir exactement 67.68 sans artefact float (67.67999999999999)
    let res = evaluate_math_expression("1.2*56.4").expect("Should evaluate 1.2*56.4");
    assert_eq!(res, 67.68, "1.2*56.4 doit être exactement 67.68 et non 67.67999999999999");

    // Support des séparateurs décimaux français (virgule)
    let res_comma = evaluate_math_expression("1,2 * 56,4").expect("Should evaluate 1,2 * 56,4 with commas");
    assert_eq!(res_comma, 67.68, "1,2*56,4 avec virgules doit valoir 67.68");

    // Autres cas classiques de précision IEEE-754
    let res_add = evaluate_math_expression("0.1 + 0.2").expect("Should evaluate 0.1 + 0.2");
    assert_eq!(res_add, 0.3, "0.1 + 0.2 doit être exactement 0.3 et non 0.30000000000000004");

    let res_sub = evaluate_math_expression("0.3 - 0.1").expect("Should evaluate 0.3 - 0.1");
    assert_eq!(res_sub, 0.2, "0.3 - 0.1 doit être exactement 0.2");

    let res_mult = evaluate_math_expression("35.7 * 100").expect("Should evaluate 35.7 * 100");
    assert_eq!(res_mult, 3570.0, "35.7 * 100 doit être exactement 3570.0");

    let res_diff = evaluate_math_expression("1.2 * 56.4 - 67.68").expect("Should evaluate chained subtraction");
    assert_eq!(res_diff, 0.0, "1.2 * 56.4 - 67.68 doit être exactement 0.0");
}

#[test]
fn test_prod_02_evaluate_nested_parentheses_math() {
    let res = evaluate_math_expression("((10 + 20) * 3) / 2").expect("Should evaluate parentheses");
    assert!((res - 45.0).abs() < 1e-6);

    let res_pow = evaluate_math_expression("2 ^ 8").expect("Should evaluate exponentiation");
    assert!((res_pow - 256.0).abs() < 1e-6);
}

#[test]
fn test_prod_03_evaluate_invalid_math_returns_error() {
    assert!(evaluate_math_expression("Bonjour le monde").is_err());
    assert!(evaluate_math_expression("10 / 0").is_err());
    assert!(evaluate_math_expression("10 +").is_err());
}

#[test]
fn test_prod_04_append_todo_to_inbox() {
    let dir = tempdir().expect("Failed to create tempdir");
    let vault_path = dir.path();

    let res = append_todo(vault_path, "Acheter des câbles HDMI").expect("Should append todo");
    assert!(res.contains("Inbox.md"));

    let inbox_content =
        std::fs::read_to_string(vault_path.join("Inbox.md")).expect("Should read Inbox.md");
    assert!(inbox_content.contains("- [ ] "));
    assert!(inbox_content.contains("Acheter des câbles HDMI"));
}

#[test]
fn test_prod_05_extract_and_toggle_task() {
    let dir = tempdir().expect("Failed to create tempdir");
    let vault_path = dir.path();
    let inbox_file = vault_path.join("Inbox.md");

    std::fs::write(
        &inbox_file,
        "# Inbox\n\n- [ ] Première tâche urgente\n- [ ] Deuxième tâche\n",
    )
    .expect("Should write inbox");

    let tasks = extract_tasks_from_file(&inbox_file).expect("Should extract tasks");
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].content, "Première tâche urgente");
    assert!(!tasks[0].checked);

    toggle_task_in_file(&inbox_file, tasks[0].line_number, true).expect("Should toggle task");

    let updated_tasks = extract_tasks_from_file(&inbox_file).expect("Should extract updated tasks");
    assert!(updated_tasks[0].checked);
    assert!(!updated_tasks[1].checked);
}

#[test]
fn test_prod_06_append_log_entry() {
    let dir = tempdir().expect("Failed to create tempdir");
    let vault_path = dir.path();

    let res = append_log_entry(vault_path, "Appel client Alpha").expect("Should append log");
    assert!(res.contains("Journal"));

    let files = std::fs::read_dir(vault_path.join("Journal")).expect("Journal dir should exist");
    let journal_file = files
        .into_iter()
        .next()
        .expect("At least one journal file")
        .expect("Valid entry")
        .path();

    let content = std::fs::read_to_string(journal_file).expect("Should read journal");
    assert!(content.contains("Appel client Alpha"));
}

#[test]
fn test_prod_07_create_meeting_note() {
    let dir = tempdir().expect("Failed to create tempdir");
    let vault_path = dir.path();

    let res =
        create_meeting_note(vault_path, "Sprint Review Q3").expect("Should create meeting note");
    assert!(res.contains("Reunions"));
    assert!(res.contains("Sprint Review Q3"));

    let content = std::fs::read_to_string(&res).expect("Should read meeting file");
    assert!(content.contains("# Réunion : Sprint Review Q3"));
    assert!(content.contains("## Participants"));
    assert!(content.contains("## Ordre du jour"));
    assert!(content.contains("## Actions à mener"));
}

#[test]
fn test_prod_08_append_bookmark() {
    let dir = tempdir().expect("Failed to create tempdir");
    let vault_path = dir.path();

    let res = append_bookmark(
        vault_path,
        "https://rust-lang.org",
        Some("Site officiel du langage Rust".to_string()),
    )
    .expect("Should append bookmark");
    assert!(res.contains("Bookmarks.md"));

    let content = std::fs::read_to_string(&res).expect("Should read bookmarks");
    assert!(content.contains("https://rust-lang.org"));
    assert!(content.contains("Site officiel du langage Rust"));
}
