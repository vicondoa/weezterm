// --- weezterm remote features ---
// TODO(rendering-cleanup): once Mode C (SoftwareRdp, see
// docs/windows-rendering-design.md §4) has shipped for one full
// release, this LLVMpipe-via-SWRAST path becomes redundant — Mode C
// handles the RDP / virtualised-GPU case more efficiently (WARP
// D3D11 + Present1 dirty rects, no GPU readback per frame). Track
// removal in the phase-7 follow-up once the release ships. Until
// then it stays as a safety net for the OpenGL front-end on RDP.
// --- end weezterm remote features ---
pub(crate) fn prefer_swrast() -> bool {
    #[cfg(windows)]
    {
        if crate::os::windows::is_running_in_rdp_session() {
            // Using OpenGL in RDP has problematic behavior upon
            // disconnect, so we force the use of software rendering.
            log::trace!("Running in an RDP session, use SWRAST");
            return true;
        }
    }
    config::configuration().front_end == config::FrontEndSelection::Software
}
