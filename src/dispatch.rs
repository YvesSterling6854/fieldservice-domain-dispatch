use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    AwaitingDomain,
    ReadyForDispatch,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkOrderPhoto {
    pub object_key: String,
    pub caption: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TechnicianFollowUp {
    pub technician_id: String,
    pub note: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkOrder {
    pub work_order_id: String,
    pub customer_domain: String,
    pub photos: Vec<WorkOrderPhoto>,
    pub follow_up: Option<TechnicianFollowUp>,
    pub dispatch_status: DispatchStatus,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VerificationEvent {
    pub event: String,
    pub domain: String,
}

pub fn apply_verification(order: &mut WorkOrder, event: &VerificationEvent) -> bool {
    if event.event == "dns.domain.verified"
        && event.domain == order.customer_domain
        && order.dispatch_status == DispatchStatus::AwaitingDomain
    {
        order.dispatch_status = DispatchStatus::ReadyForDispatch;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_verification_releases_the_work_order_and_preserves_evidence() {
        let mut order = WorkOrder {
            work_order_id: "wo-1842".into(),
            customer_domain: "dispatch.acme-field.example".into(),
            photos: vec![WorkOrderPhoto {
                object_key: "wo-1842/panel-before.jpg".into(),
                caption: "Panel before service".into(),
            }],
            follow_up: Some(TechnicianFollowUp {
                technician_id: "tech-17".into(),
                note: "Confirm breaker label on return visit".into(),
            }),
            dispatch_status: DispatchStatus::AwaitingDomain,
        };

        let changed = apply_verification(
            &mut order,
            &VerificationEvent {
                event: "dns.domain.verified".into(),
                domain: "dispatch.acme-field.example".into(),
            },
        );

        assert!(changed);
        assert_eq!(order.dispatch_status, DispatchStatus::ReadyForDispatch);
        assert_eq!(order.photos.len(), 1);
        assert!(order.follow_up.is_some());
    }

    #[test]
    fn unrelated_domain_does_not_release_the_work_order() {
        let mut order = WorkOrder {
            work_order_id: "wo-1842".into(),
            customer_domain: "dispatch.acme-field.example".into(),
            photos: vec![],
            follow_up: None,
            dispatch_status: DispatchStatus::AwaitingDomain,
        };
        let changed = apply_verification(
            &mut order,
            &VerificationEvent {
                event: "dns.domain.verified".into(),
                domain: "dispatch.other.example".into(),
            },
        );
        assert!(!changed);
        assert_eq!(order.dispatch_status, DispatchStatus::AwaitingDomain);
    }
}
