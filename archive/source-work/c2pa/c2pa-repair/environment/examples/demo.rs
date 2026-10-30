use c2pa_repair::*;
fn main() {
    let input=RepairInput{
        asset:Asset{id:"asset-7".into(),mime:"image/x-synthetic".into(),hash:"a7".into()},
        prior_store:vec![PriorManifest{claim_id:"claim-old".into(),asset_id:"asset-7".into()}],
        malformed:UpdateManifest{claim_id:"claim-new".into(),asset_id:"asset-7".into(),assertions:vec![
            Assertion{id:"act".into(),label:"c2pa.actions".into(),kind:AssertionKind::Action{value:"opened".into()}},
            Assertion{id:"ing".into(),label:"c2pa.ingredient".into(),kind:AssertionKind::Ingredient{relationship:"parent".into(),manifest_ref:"claim-old".into()}},
        ]},
        legal_edits:vec![Edit::ReplaceAction{target:"act".into(),value:"edited".into()}],
    };
    println!("{:?}",repair(&input));
}
