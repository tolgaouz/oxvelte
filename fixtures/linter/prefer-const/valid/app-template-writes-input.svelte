<script>
	export let prop = 1;
	let bound = 0;
	let assigned = 0;
</script>

<input bind:value={bound} oninput={() => { assigned = 2; }} />
{prop}
