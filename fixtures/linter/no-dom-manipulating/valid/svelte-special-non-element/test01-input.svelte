<script>
	let body;

	function mutate() {
		body.remove();
	}
</script>

<svelte:body bind:this={body} />
