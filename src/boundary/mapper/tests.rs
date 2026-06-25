use super::*;
use crate::boundary::effect::{BridgeEffectKind, BridgeEventContext, IgnoredCodexEventReason};
use codex_protocol::{
    models::ResponseItem,
    protocol::{EventMsg, Op, ReviewDecision, RolloutItem},
};

#[test]
fn classifies_actor_owned_request_user_input_by_context() {
    let event =
        EventMsg::RequestUserInput(codex_protocol::request_user_input::RequestUserInputEvent {
            call_id: "call".to_string(),
            turn_id: "turn".to_string(),
            auto_resolution_ms: None,
            questions: vec![],
        });

    assert_eq!(
        classify_event_msg(&event, BridgeEventContext::Live),
        BridgeEffectKind::Ignore(IgnoredCodexEventReason::HandledByActor)
    );
    assert_eq!(
        classify_event_msg(&event, BridgeEventContext::Replay),
        BridgeEffectKind::Forward
    );
}

#[test]
fn classifies_user_message_by_context() {
    let event = EventMsg::UserMessage(codex_protocol::protocol::UserMessageEvent {
        message: "hello".to_string(),
        images: None,
        image_details: Vec::new(),
        text_elements: Vec::new(),
        local_images: Vec::new(),
        local_image_details: Vec::new(),
        client_id: None,
    });

    assert_eq!(
        classify_event_msg(&event, BridgeEventContext::Live),
        BridgeEffectKind::Ignore(IgnoredCodexEventReason::AlreadyRenderedByClientInput)
    );
    assert_eq!(
        classify_event_msg(&event, BridgeEventContext::Replay),
        BridgeEffectKind::Forward
    );
}

#[test]
fn classifies_response_items_without_fallback() {
    assert_eq!(
        classify_response_item(&ResponseItem::Other),
        BridgeEffectKind::Ignore(IgnoredCodexEventReason::UnsupportedByAcp)
    );
}

#[test]
fn classifies_rollout_items_without_fallback() {
    let item = RolloutItem::Compacted(codex_protocol::protocol::CompactedItem {
        message: "summary".to_string(),
        replacement_history: None,
        window_number: None,
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
    });

    assert_eq!(
        classify_rollout_item(&item),
        BridgeEffectKind::Ignore(IgnoredCodexEventReason::StateOnly)
    );
}

#[test]
fn routes_live_request_user_input_to_actor_owned_ignore() {
    let event =
        EventMsg::RequestUserInput(codex_protocol::request_user_input::RequestUserInputEvent {
            call_id: "call".to_string(),
            turn_id: "turn".to_string(),
            auto_resolution_ms: None,
            questions: vec![],
        });

    let LiveEventRoute::Ignore { reason, .. } = route_live_event(event) else {
        panic!("expected live request user input to be ignored by the boundary");
    };

    assert_eq!(reason, IgnoredCodexEventReason::HandledByActor);
}

#[test]
fn plans_actor_owned_request_user_input() {
    let event =
        EventMsg::RequestUserInput(codex_protocol::request_user_input::RequestUserInputEvent {
            call_id: "call".to_string(),
            turn_id: "turn".to_string(),
            auto_resolution_ms: None,
            questions: vec![],
        });

    let plan = plan_actor_event(&event);
    let ActorEventAction::RegisterPendingUserInput(request) = plan.action else {
        panic!("expected actor to own request user input");
    };

    assert_eq!(request.call_id, "call");
    assert!(plan.state_updates.is_empty());
}

#[test]
fn plans_full_access_patch_auto_approval_as_submit_op() {
    let event =
        EventMsg::ApplyPatchApprovalRequest(crate::test_fixtures::apply_patch_approval_request(
            "patch-call",
            "turn",
            std::collections::HashMap::new(),
            None,
        ));

    let plan = plan_actor_event(&event);
    let ActorEventAction::RouteToSubmission {
        clear_pending_user_input,
        full_access_auto_approval: Some(auto_approval),
        ..
    } = plan.action
    else {
        panic!("expected patch approval to expose full-access auto approval");
    };

    assert_eq!(clear_pending_user_input, ActorPendingUserInputClear::None);
    assert!(matches!(
        auto_approval.into_op(),
        Op::PatchApproval {
            id,
            decision: ReviewDecision::Approved,
        } if id == "patch-call"
    ));
}

#[test]
fn routes_reasoning_raw_delta_through_normalized_delta_event() {
    let event = EventMsg::ReasoningRawContentDelta(
        codex_protocol::protocol::ReasoningRawContentDeltaEvent {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            item_id: "item".to_string(),
            delta: "thinking".to_string(),
            content_index: 2,
        },
    );

    let LiveEventRoute::Forward(LiveForwardEvent::ReasoningContentDelta(delta)) =
        route_live_event(event)
    else {
        panic!("expected raw reasoning delta to be forwarded as normalized reasoning content");
    };

    assert_eq!(delta.thread_id, "thread");
    assert_eq!(delta.turn_id, "turn");
    assert_eq!(delta.item_id, "item");
    assert_eq!(delta.index, 2);
    assert_eq!(delta.delta, "thinking");
}
