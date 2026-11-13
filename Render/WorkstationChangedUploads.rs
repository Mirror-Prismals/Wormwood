//! Exact changed-block uploads for the packed optical packet. No data is
//! quantized or omitted on first upload, buffer replacement, or asset writes.
use super::super::workstation as packet;
const BLOCK: usize = 128;
pub(super) fn ranges<'a>(rows: &'a [[f32;4]], previous: &[[f32;4]]) -> Vec<(usize,&'a [[f32;4]])> {
    let reusable=rows.len()==packet::FRAME_ROWS&&previous.len()==rows.len();
    let mut result=Vec::new();
    let mut source=0;
    for (destination,range) in packet::upload_ranges(rows) {
        if !reusable {result.push((destination,range));source+=range.len();continue;}
        let mut first=None;
        for at in (0..range.len()).step_by(BLOCK) {
            let end=(at+BLOCK).min(range.len());
            let same=range[at..end].iter().zip(&previous[source+at..source+end])
                .all(|(a,b)|a.map(f32::to_bits)==b.map(f32::to_bits));
            if !same {first.get_or_insert(at);}
            else if let Some(start)=first.take(){result.push((destination+start,&range[start..at]));}
        }
        if let Some(start)=first {result.push((destination+start,&range[start..]));}
        source+=range.len();
    }
    result
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn packed_updates_reconstruct_exact_gpu_contents_without_touching_immutable_gaps() {
        use packet::{FRAME_ROWS,STORAGE_BYTES,STATIC_FRAME_ROWS};
        let old=vec![[0.;4];FRAME_ROWS];let mut new=old.clone();
        for at in [0,127,128,STATIC_FRAME_ROWS-1,STATIC_FRAME_ROWS,crate::workstation::piano_scene::BASE,FRAME_ROWS-1] {
            new[at]=[at as f32,-0.,f32::from_bits(1),0.25];
        }
        let mut expected=vec![[7f32.to_bits();4];STORAGE_BYTES/16];let mut actual=expected.clone();
        for (at,values) in packet::upload_ranges(&old) {
            for (dst,src) in actual[at..at+values.len()].iter_mut().zip(values){*dst=src.map(f32::to_bits);}
        }
        for (at,values) in packet::upload_ranges(&new) {
            for (dst,src) in expected[at..at+values.len()].iter_mut().zip(values){*dst=src.map(f32::to_bits);}
        }
        let changed=ranges(&new,&old);assert!(changed.iter().map(|(_,r)|r.len()).sum::<usize>()<FRAME_ROWS/4);
        for (at,values) in changed {
            for (dst,src) in actual[at..at+values.len()].iter_mut().zip(values){*dst=src.map(f32::to_bits);}
        }
        assert_eq!(actual,expected);
        assert!(ranges(&new,&new).is_empty());
        assert_eq!(ranges(&new,&[]).iter().map(|(_,r)|r.len()).sum::<usize>(),FRAME_ROWS);
        assert_eq!(ranges(&new[..256],&old).len(),1);
    }
}
