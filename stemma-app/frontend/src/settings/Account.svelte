<script lang="ts">
  import { app } from "../lib/app.svelte";
</script>

<h3 class="settings__title">Account</h3>
<dl class="facts">
  <div><dt>Signed in as</dt><dd class="identifier">{app.signed?.user}</dd></div>
  <div><dt>Server</dt><dd class="identifier">{app.signed?.server}</dd></div>
</dl>
<div class="setting">
  <button type="button" class="button" onclick={() => app.signOut()}>Sign out of this machine</button>
  <p class="setting__note">Signing out leaves your work on this machine. It is read again when you sign back in.</p>
</div>
