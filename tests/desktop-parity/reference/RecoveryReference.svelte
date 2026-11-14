<script lang="ts">
import App from "../../../apps/desktop/src/App.svelte";
import RecoveryKitModal from "../../../apps/desktop/src/lib/components/auth/RecoveryKitModal.svelte";
const recoveryKey="AIPASS-ABCD-EFGH-JKLM-NPQR-STUV-WXYZ";
</script>
<App />
<RecoveryKitModal {recoveryKey} />
