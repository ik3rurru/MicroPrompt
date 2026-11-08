"""Tests for the decisions apuntador.py makes without Hex or Ollama: when an
interviewer's turn is a question, and whether it is one of the guion's.

    py -m unittest -v test_apuntador.py
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path

import apuntador

HERE = Path(__file__).resolve().parent


class GuionTest(unittest.TestCase):
    def setUp(self):
        self.guion = apuntador.parse_guion((HERE / "ejemplo" / "guion.md").read_text(encoding="utf-8"))

    def test_parses_prepared_questions_and_closing_questions(self):
        names = [p.name for p in self.guion["prepared"]]
        self.assertIn("¿Cuáles son tus expectativas salariales?", names)
        self.assertEqual(names[-1], "Preguntas que haré")
        for entry in self.guion["prepared"]:
            self.assertEqual(len(entry.points), 3, entry.name)

    def test_template_parses_without_points(self):
        guion = apuntador.parse_guion((HERE / "plantilla-guion.md").read_text(encoding="utf-8"))
        self.assertEqual(len(guion["prepared"]), 4)  # three questions and the closing one

    def test_every_scripted_question_gets_the_right_answer_source(self):
        """Reworded prepared questions find their entry; unexpected ones find none."""
        turns = json.loads((HERE / "ejemplo" / "entrevista.json").read_text(encoding="utf-8"))["turnos"]
        for turn in turns:
            if turn["tipo"] == "no-pregunta":
                continue
            entry, score = apuntador.match_prepared(turn["texto"], self.guion["prepared"])
            with self.subTest(turn["texto"]):
                self.assertEqual(entry and entry.name, turn.get("esperada"), score)

    def test_tienes_alguna_is_not_always_the_closing_question(self):
        for text in ("¿Tienes alguna experiencia con Kafka?", "¿Tienes alguna certificación?",
                     "Tienes alguna duda técnica sobre Kubernetes"):
            entry, _ = apuntador.match_prepared(text, self.guion["prepared"])
            self.assertIsNone(entry, text)
        entry, _ = apuntador.match_prepared("¿Tienes alguna pregunta?", self.guion["prepared"])
        self.assertEqual(entry.name, "Preguntas que haré")

    def test_tolerates_asr_without_punctuation_or_accents(self):
        entry, _ = apuntador.match_prepared("cuanto te gustaria cobrar", self.guion["prepared"])
        self.assertEqual(entry.name, "¿Cuáles son tus expectativas salariales?")


class QuestionTest(unittest.TestCase):
    def test_questions_and_requests(self):
        for text in ("¿Me haces un resumen de tu trayectoria?", "Cuéntame una situación difícil.",
                     "Y qué harías si te cambian las prioridades", "Háblame de un proyecto.",
                     "Vale. Por qué quieres cambiar de empresa",
                     # Nemotron's text as it came out of Hex, without question marks:
                     "Para empezar, me haces un pequeño resumen de tu trayectoria profesional.",
                     "Como diseñarías un sistema para que los repartidores reciban los cambios de ruta.",
                     "Tienes alguna pregunta para nosotros.",
                     "Me gustaría que me contaras cómo trabajas en equipo.",
                     "Quería preguntarte por tu último proyecto.",
                     "Quería si a mitad de un sprint te cambian las prioridades."):
            self.assertTrue(apuntador.is_question(text), text)

    def test_statements_are_not_questions(self):
        for text in ("Vale, perfecto, muy interesante.",
                     "Te explico cómo va a ser la entrevista: primero hablamos de ti.",
                     "Genial. Te cuento un poco del puesto: el equipo son seis personas.",
                     "Te cuento un poco del puesto, el equipo son seis personas. Trabajamos con paiza ni"
                     " kubernetes, y ahora mismo estamos migrando el sistema de rutas a eventos.",
                     "Hola, buenos días, gracias por venir.", "Bueno.", "Pues por nuestra parte eso es todo.",
                     "Como te decía, somos seis personas.", "Cuando quieras empezamos.", "Tienes razón.",
                     "Que vaya bien.", "Historias como esa nos encantan."):
            self.assertFalse(apuntador.is_question(text), text)

    def test_a_turn_split_by_a_pause_fires_with_its_whole_text(self):
        turns = apuntador.Turns(gap_ms=5000)
        self.assertIsNone(turns.add("Hablemos de dinero.", 1800))
        self.assertEqual(turns.add("¿Cuánto te gustaría cobrar?", 4500),
                         "Hablemos de dinero. ¿Cuánto te gustaría cobrar?")
        # After a long gap (the candidate answered), a new turn starts.
        self.assertEqual(turns.add("¿Y en cinco años?", 13500), "¿Y en cinco años?")

    def test_a_question_cut_by_a_pause_fires_again_whole(self):
        turns = apuntador.Turns(gap_ms=5000)
        self.assertEqual(turns.add("Cuéntame qué harías", 0), "Cuéntame qué harías")
        self.assertEqual(turns.add("si te cambian las prioridades.", 2000),
                         "Cuéntame qué harías si te cambian las prioridades.")


class PointsTest(unittest.TestCase):
    def test_clean_point(self):
        self.assertEqual(apuntador.clean_point("- Kafka: -40 % incidencias"), "Kafka: -40 % incidencias")
        self.assertEqual(apuntador.clean_point("2. **Medir antes**"), "Medir antes")
        self.assertEqual(apuntador.clean_point("   "), "")


if __name__ == "__main__":
    unittest.main()
