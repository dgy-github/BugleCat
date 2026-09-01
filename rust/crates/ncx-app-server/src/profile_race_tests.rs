use super::*;

#[test]
fn harness_profile_change_loses_to_a_first_turn_that_starts_during_validation() {
    let server = Arc::new(server());
    let setup_runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("profile-first-turn-race").unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadCreateActivate {
                thread_id: thread_id.clone(),
                workspace: "workspace".into(),
                title: "title".into(),
                harness_profile: "full".into(),
            },
            &setup_runtime,
        )
        .unwrap();

    // Stop validation after it has read the durable workspace but before it
    // reaches the atomic profile update. This reproduces a user sending the
    // first turn while a profile picker request is still in flight.
    let gate = Arc::new(ProfileValidationGate::default());
    let runtime = Arc::new(RecordingRuntime {
        profile_validation_gate: Some(gate.clone()),
        ..Default::default()
    });
    let profile_server = server.clone();
    let profile_thread = thread_id.clone();
    let profile_runtime = runtime.clone();
    let pending_profile = std::thread::spawn(move || {
        profile_server.dispatch_with_runtime(
            ClientRequest::ThreadHarnessProfileSet {
                thread_id: profile_thread,
                harness_profile: "readonly".into(),
            },
            profile_runtime.as_ref(),
        )
    });

    gate.wait_until_entered();
    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread_id.clone(),
            turn_id: TurnId::new("first-turn").unwrap(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .unwrap();
    gate.release();

    let error = pending_profile.join().unwrap().unwrap_err();
    assert!(matches!(
        error,
        AppServerError::InvalidRequest(ref message)
            if message == "Harness Profile is locked after the first turn"
    ));
    let read = server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .unwrap();
    assert!(matches!(
        read.response.payload,
        ResponsePayload::Thread(ref thread)
            if thread.metadata.harness_profile == "full" && thread.turns.len() == 1
    ));
}
