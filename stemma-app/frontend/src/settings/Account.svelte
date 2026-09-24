<script lang="ts">
  import { app } from "../lib/app.svelte";
</script>

<h2>Account</h2>
<div class="field"><p class="label">Signed in as</p><p class="row__name">{app.signed?.user}</p></div>
<div class="field"><p class="label">Instance</p><p class="quiet identifier">{app.signed?.server}</p></div>
<div class="row__actions"><button onclick={() => app.signOut()}>Sign out of this machine</button></div>
<p class="quiet">Signing out leaves your work on this machine. It is read again when you sign back in.</p>
