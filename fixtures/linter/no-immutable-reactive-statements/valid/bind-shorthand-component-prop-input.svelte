<script>
	let prompt = { seed: '1' };

	$: empty = Number(prompt.seed);
</script>

<Prompt bind:prompt />
