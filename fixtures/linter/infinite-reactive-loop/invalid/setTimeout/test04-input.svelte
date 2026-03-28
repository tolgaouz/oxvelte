<script>
	let a = 0, b = 1;

	$: setTimeout(() => {
		b = b + 1;
	}, 100);
</script>
