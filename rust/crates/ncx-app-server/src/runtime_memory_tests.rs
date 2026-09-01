use super::*;

#[test]
fn memory_service_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryList {
                    workspace: "D:\\project-before-switch".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::MemoryNotes(ref value) if value.is_array()
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryAdd {
                    note: "remember".into(),
                    tags: vec!["project".into()],
                    workspace: "D:\\project-before-switch".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::Bool(true)
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryConsolidate {
                    workspace: "D:\\project-before-switch".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::Count(2)
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryMergeStart {
                    workspace: "D:\\project-before-switch".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::MemoryMergeOperation(ref value) if value["status"] == "running"
    ));
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "memory-list:D:\\project-before-switch",
            "memory-add:remember:1:D:\\project-before-switch",
            "memory-consolidate:D:\\project-before-switch",
            "memory-merge-start:D:\\project-before-switch",
        ]
    );
}

#[test]
fn job_status_and_cancellation_keep_workspace_and_generation_at_the_adapter_boundary() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let workspace = "D:\\project-before-switch".to_string();

    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryMergeStatusRead {
                    workspace: workspace.clone(),
                    generation: Some(7),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::MemoryMergeOperation(ref value)
            if value["generation"] == 7 && value["status"] == "idle"
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryMergeCancel {
                    workspace: workspace.clone(),
                    generation: 7,
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::MemoryMergeOperation(ref value)
            if value["generation"] == 7 && value["status"] == "cancelling"
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::ForgeJobStatusRead {
                    workspace: workspace.clone(),
                    generation: None,
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::ForgeJob(ref value)
            if value["generation"] == 0 && value["status"] == "idle"
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::ForgeJobCancel {
                    workspace: workspace.clone(),
                    generation: 9,
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::ForgeJob(ref value)
            if value["generation"] == 9 && value["status"] == "cancelling"
    ));
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "memory-merge-status:D:\\project-before-switch:7",
            "memory-merge-cancel:D:\\project-before-switch:7",
            "forge-status:D:\\project-before-switch:",
            "forge-cancel:D:\\project-before-switch:9",
        ]
    );
}
