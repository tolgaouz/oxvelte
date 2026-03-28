<script>
	let a = 0;

	$: (async () => {
		const msg = "please await for it";
		a = msg.length;
	})();
</script>
