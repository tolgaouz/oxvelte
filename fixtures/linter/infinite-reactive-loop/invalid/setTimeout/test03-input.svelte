<script>
	let { count, name } = getStore();

	$: setTimeout(() => {
		count = count + 1;
	}, 100);
</script>
