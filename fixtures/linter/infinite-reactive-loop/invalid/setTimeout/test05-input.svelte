<script>
	const obj = { a: 0 };

	$: setTimeout(() => {
		obj.a = 1;
	}, 100);
</script>
