/* Reusable retrieval-practice quiz widget. Markup:
   <div class="quiz" data-explain="why...">
     <p class="q">Question?</p>
     <button data-correct>Right answer</button>
     <button>Wrong answer</button>
   </div>
   Immediate feedback; storage-strength via effortful recall, so no hints in the
   styling — every option looks identical until clicked. */
document.querySelectorAll(".quiz").forEach((quiz) => {
  const buttons = quiz.querySelectorAll("button");
  const explain = quiz.dataset.explain;
  const fb = document.createElement("p");
  fb.className = "quiz-feedback";
  quiz.appendChild(fb);
  buttons.forEach((btn) => {
    btn.addEventListener("click", () => {
      const correct = btn.hasAttribute("data-correct");
      buttons.forEach((b) => {
        b.disabled = true;
        if (b.hasAttribute("data-correct")) b.classList.add("right");
        else if (b === btn) b.classList.add("wrong");
      });
      fb.innerHTML =
        (correct ? "✓ Correct. " : "✗ Not quite. ") + (explain || "");
      fb.classList.add(correct ? "ok" : "no");
    });
  });
});
