/// Missing selection retains the legacy single-depth/automatic IPC behavior.
pub(crate) fn normalize(
    depths: Option<&[u8]>,
    legacy_depth: Option<u8>,
) -> Result<Option<Vec<u8>>, String> {
    let Some(depths) = depths else {
        return Ok(legacy_depth.map(|d| vec![d]));
    };
    if legacy_depth.is_some() {
        return Err("provide either depths or maxDepth, not both".into());
    }
    if depths.is_empty() || depths.len() > 256 {
        return Err("select between 1 and 256 depths (0–255), or use auto".into());
    }
    let mut result = depths.to_vec();
    result.sort_unstable();
    result.dedup();
    Ok(Some(result))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_normalizes_depth_requests() {
        assert_eq!(normalize(None, None).unwrap(), None);
        assert_eq!(normalize(None, Some(17)).unwrap(), Some(vec![17]));
        assert_eq!(
            normalize(Some(&[17, 0, 12, 13, 12, 255]), None).unwrap(),
            Some(vec![0, 12, 13, 17, 255])
        );
        assert!(normalize(Some(&[]), None).is_err());
        assert!(normalize(Some(&[0; 257]), None).is_err());
        assert!(normalize(Some(&[17]), Some(17)).is_err());
    }
}
