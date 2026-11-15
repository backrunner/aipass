<script lang="ts">
import App from "../../../artifacts/desktop-reference/src/App.svelte";
import RecoveryKitModal from "../../../artifacts/desktop-reference/src/lib/components/auth/RecoveryKitModal.svelte";
const recoveryKey="AIPASS-ABCD-EFGH-JKLM-NPQR-STUV-WXYZ";
</script>
<App />
<RecoveryKitModal {recoveryKey} />
