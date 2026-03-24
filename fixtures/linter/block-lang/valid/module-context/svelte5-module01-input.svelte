<script module lang="ts">
  // Svelte 5 module script using TypeScript
  export const prerender = true;
</script>

<script lang="ts">
  let count = $state(0);
</script>

<button onclick={() => count++}>{count}</button>
